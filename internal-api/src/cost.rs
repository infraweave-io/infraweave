//! Deployment cost requests. A deployment's cost is attributed through the
//! `INFRAWEAVE_DEPLOYMENT_ID` tag the runner puts on all of its resources, so the tag
//! must be activated as a cost allocation tag in the billing account.

use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Months, NaiveDate, NaiveDateTime, Timelike, Utc};
use serde_json::Value;

use env_aws_direct::{CostFilters, CostGroupBy, DEPLOYMENT_ID_COST_TAG};

use crate::http_response::bad_request;

const DEFAULT_GROUP_BY: &str = "SERVICE";

/// Cost Explorer dimensions a deployment's cost can be broken down by.
const GROUP_BY_DIMENSIONS: &[&str] = &[
    "SERVICE",
    "USAGE_TYPE",
    "OPERATION",
    "REGION",
    "AZ",
    "INSTANCE_TYPE",
    "LINKED_ACCOUNT",
    "PURCHASE_TYPE",
    "RECORD_TYPE",
    "PLATFORM",
    "TENANCY",
    "DATABASE_ENGINE",
    "BILLING_ENTITY",
    "LEGAL_ENTITY_NAME",
];

/// Cost Explorer accepts at most two group-by keys.
const MAX_GROUP_BY: usize = 2;

/// The first is the default. Amortized spreads Savings Plan and Reserved Instance commitments
/// over the usage they cover, so covered usage is not near zero as it is unblended; without
/// commitments the two are the same.
const METRICS: &[&str] = &[
    "AmortizedCost",
    "UnblendedCost",
    "BlendedCost",
    "NetUnblendedCost",
    "NetAmortizedCost",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CostGranularity {
    Monthly,
    Daily,
    Hourly,
}

impl CostGranularity {
    fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_uppercase().as_str() {
            "MONTHLY" => Ok(Self::Monthly),
            "DAILY" => Ok(Self::Daily),
            "HOURLY" => Ok(Self::Hourly),
            _ => Err(bad_request(format!(
                "Invalid granularity '{}'. Expected MONTHLY, DAILY or HOURLY",
                value
            ))),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Monthly => "MONTHLY",
            Self::Daily => "DAILY",
            Self::Hourly => "HOURLY",
        }
    }

    fn default_periods(self) -> i64 {
        match self {
            Self::Monthly => 6,
            Self::Daily => 30,
            Self::Hourly => 24,
        }
    }

    /// Cost Explorer keeps hourly data for 14 days and daily and monthly data for at most
    /// 38 months (14 unless multi-year data is enabled; it rejects the rest itself).
    fn max_periods(self) -> i64 {
        match self {
            Self::Monthly => 38,
            Self::Daily => 38 * 31,
            Self::Hourly => 14 * 24,
        }
    }

    fn format(self, at: NaiveDateTime) -> String {
        match self {
            Self::Hourly => at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            Self::Monthly | Self::Daily => at.format("%Y-%m-%d").to_string(),
        }
    }

    /// The start of the period `at` falls in.
    fn truncate(self, at: NaiveDateTime) -> NaiveDateTime {
        let day = at.date().and_hms_opt(0, 0, 0).unwrap_or(at);
        match self {
            Self::Hourly => day + Duration::hours(at.hour() as i64),
            Self::Daily => day,
            Self::Monthly => day.with_day(1).unwrap_or(day),
        }
    }

    fn add_periods(self, at: NaiveDateTime, periods: i64) -> NaiveDateTime {
        match self {
            Self::Hourly => at + Duration::hours(periods),
            Self::Daily => at + Duration::days(periods),
            Self::Monthly if periods >= 0 => at + Months::new(periods as u32),
            Self::Monthly => at - Months::new(periods.unsigned_abs() as u32),
        }
    }

    fn periods_between(self, start: NaiveDateTime, end: NaiveDateTime) -> i64 {
        match self {
            Self::Hourly => (end - start).num_hours(),
            Self::Daily => (end - start).num_days(),
            Self::Monthly => {
                (end.year() - start.year()) as i64 * 12 + end.month() as i64 - start.month() as i64
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CostRequest {
    pub granularity: CostGranularity,
    /// Inclusive, in Cost Explorer's format for the granularity.
    pub start: String,
    /// Exclusive, in Cost Explorer's format for the granularity.
    pub end: String,
    pub metric: String,
    pub group_by: Vec<String>,
}

/// Reads the cost options from a request payload:
/// - `granularity`: `MONTHLY`, `DAILY` (default) or `HOURLY`.
/// - `start` / `end`: `YYYY-MM-DD` or an RFC 3339 time, `end` exclusive. Without `end` the range
///   runs through the current period; without `start` it covers `periods` periods.
/// - `periods`: how many periods back `start` defaults to (6 months, 30 days or 24 hours).
/// - `group_by`: comma-separated dimensions, `SERVICE` by default; empty for totals only.
/// - `metric`: `AmortizedCost` (default), `UnblendedCost`, `BlendedCost`, ...
pub(crate) fn parse_cost_request(payload: &Value, now: DateTime<Utc>) -> Result<CostRequest> {
    let granularity = match param(payload, "granularity") {
        Some(value) => CostGranularity::parse(&value)?,
        None => CostGranularity::Daily,
    };

    let end = match param(payload, "end") {
        Some(value) => granularity.truncate(parse_time(&value, "end")?),
        None => granularity.add_periods(granularity.truncate(now.naive_utc()), 1),
    };
    let start = match param(payload, "start") {
        Some(value) => granularity.truncate(parse_time(&value, "start")?),
        None => {
            let periods = match param(payload, "periods") {
                Some(value) => value
                    .parse::<i64>()
                    .ok()
                    .filter(|p| (1..=granularity.max_periods()).contains(p))
                    .ok_or_else(|| {
                        bad_request(format!(
                            "Invalid periods '{}'. Expected 1 to {} for {} granularity",
                            value,
                            granularity.max_periods(),
                            granularity.as_str()
                        ))
                    })?,
                None => granularity.default_periods(),
            };
            granularity.add_periods(end, -periods)
        }
    };

    if start >= end {
        return Err(bad_request("'start' must be before 'end'"));
    }
    if granularity.periods_between(start, end) > granularity.max_periods() {
        return Err(bad_request(format!(
            "The range spans more than {} periods, the most Cost Explorer returns for {} granularity",
            granularity.max_periods(),
            granularity.as_str()
        )));
    }

    let metric = match param(payload, "metric") {
        Some(value) => METRICS
            .iter()
            .find(|m| m.eq_ignore_ascii_case(&value))
            .map(|m| m.to_string())
            .ok_or_else(|| {
                bad_request(format!(
                    "Invalid metric '{}'. Expected one of {}",
                    value,
                    METRICS.join(", ")
                ))
            })?,
        None => METRICS[0].to_string(),
    };

    let group_by = match payload.get("group_by").and_then(|v| v.as_str()) {
        Some(value) => parse_group_by(value)?,
        None => vec![DEFAULT_GROUP_BY.to_string()],
    };

    Ok(CostRequest {
        granularity,
        start: granularity.format(start),
        end: granularity.format(end),
        metric,
        group_by,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccountCostRequest {
    pub range: CostRequest,
    /// The granularity Cost Explorer is asked for. Monthly when `series=false`: the caller only
    /// wants totals, and fewer periods mean fewer (billed) pages.
    pub query_granularity: CostGranularity,
    pub group_by: Option<CostGroupBy>,
    pub filters: CostFilters,
}

/// Reads the options for the cost of the deployments in an account: the range, granularity and
/// metric as for one deployment, at most one `group_by` to break it down by (a dimension, or
/// `tag:<key>`), the `deployment_ids`, `services`, `regions` and `tag_filters` filters, and
/// `series=false` for totals only.
pub(crate) fn parse_account_cost_request(
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<AccountCostRequest> {
    let mut options = payload.clone();
    if let Some(obj) = options.as_object_mut() {
        // Parsed below: here only one dimension is allowed, and none is the default.
        obj.insert("group_by".to_string(), Value::from(""));
    }
    let range = parse_cost_request(&options, now)?;

    let group_by = match payload.get("group_by").and_then(|v| v.as_str()) {
        Some(value) => match strip_prefix_ignore_case(value.trim(), "tag:") {
            Some(key) => Some(CostGroupBy::Tag(parse_tag_key(key)?)),
            None => {
                let mut dimensions = parse_group_by(value)?;
                if dimensions.len() > 1 {
                    return Err(bad_request(
                        "Account costs are always grouped by deployment, so at most one more group_by dimension is supported",
                    ));
                }
                dimensions.pop().map(CostGroupBy::Dimension)
            }
        },
        None => None,
    };

    let totals_only = matches!(param(payload, "series").as_deref(), Some("false"));
    let query_granularity = if totals_only && range.granularity != CostGranularity::Hourly {
        CostGranularity::Monthly
    } else {
        range.granularity
    };

    Ok(AccountCostRequest {
        range,
        query_granularity,
        group_by,
        filters: parse_filters(payload)?,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResourceCostRequest {
    pub start: String,
    pub end: String,
    pub metric: String,
    pub filters: CostFilters,
}

/// Cost Explorer keeps resource-level data for the last 14 days.
const MAX_RESOURCE_DAYS: i64 = 14;

/// Reads the options for resource costs: `days` back including today (1 to 14, 14 by default),
/// `metric`, and the same filters as account costs, at least one of which must be set.
pub(crate) fn parse_resource_cost_request(
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<ResourceCostRequest> {
    let days = match param(payload, "days") {
        Some(value) => value
            .parse::<i64>()
            .ok()
            .filter(|d| (1..=MAX_RESOURCE_DAYS).contains(d))
            .ok_or_else(|| {
                bad_request(format!(
                    "Invalid days '{}'. Cost Explorer has resource costs for 1 to {} days",
                    value, MAX_RESOURCE_DAYS
                ))
            })?,
        None => MAX_RESOURCE_DAYS,
    };
    let mut options = payload.clone();
    if let Some(obj) = options.as_object_mut() {
        obj.insert("granularity".to_string(), Value::from("DAILY"));
        obj.insert("periods".to_string(), Value::from(days));
        obj.insert("group_by".to_string(), Value::from(""));
        obj.remove("start");
        obj.remove("end");
    }
    let range = parse_cost_request(&options, now)?;

    let filters = parse_filters(payload)?;
    if filters.is_empty() {
        return Err(bad_request(
            "Resource costs need deployment_ids or services to narrow them to",
        ));
    }

    Ok(ResourceCostRequest {
        start: range.start,
        end: range.end,
        metric: range.metric,
        filters,
    })
}

/// Reads the range for listing tag keys: `days` back including today, 90 by default.
pub(crate) fn parse_tag_keys_request(
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<(String, String)> {
    let mut options = payload.clone();
    if let Some(obj) = options.as_object_mut() {
        obj.insert("granularity".to_string(), Value::from("DAILY"));
        obj.insert(
            "periods".to_string(),
            Value::from(param(payload, "days").unwrap_or_else(|| "90".to_string())),
        );
        obj.insert("group_by".to_string(), Value::from(""));
        obj.remove("start");
        obj.remove("end");
    }
    let range = parse_cost_request(&options, now)?;
    Ok((range.start, range.end))
}

/// More values than this make the URL, and the Cost Explorer filter, unwieldy.
const MAX_FILTER_VALUES: usize = 200;

/// Comma-separated `deployment_ids`, `services`, `regions` and `modules`. Empty entries are dropped, so
/// `deployment_ids=,` filters on nothing rather than on an empty tag value.
fn parse_filters(payload: &Value) -> Result<CostFilters> {
    let list = |name: &str| -> Result<Vec<String>> {
        let mut values: Vec<String> = Vec::new();
        for value in param(payload, name)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if !values.iter().any(|v| v == value) {
                values.push(value.to_string());
            }
        }
        if values.len() > MAX_FILTER_VALUES {
            return Err(bad_request(format!(
                "At most {} {} can be filtered on",
                MAX_FILTER_VALUES, name
            )));
        }
        Ok(values)
    };
    Ok(CostFilters {
        deployment_ids: list("deployment_ids")?,
        services: list("services")?,
        regions: list("regions")?,
        modules: list("modules")?,
        tags: parse_tag_filters(payload)?,
    })
}

const MAX_TAG_FILTERS: usize = 10;
const MAX_TAG_KEY_LEN: usize = 128;
const MAX_TAG_VALUE_LEN: usize = 256;

fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

/// What AWS allows in a tag: letters, digits, spaces and `+ - = . _ : / @`.
fn is_tag_text(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_alphanumeric() || c == ' ' || "+-=._:/@".contains(c))
}

/// A cost allocation tag key other than the deployment id tag, which is filtered on through
/// `deployment_ids` so that an empty value can never reach Cost Explorer.
fn parse_tag_key(key: &str) -> Result<String> {
    let key = key.trim();
    if key.is_empty() || key.len() > MAX_TAG_KEY_LEN || !is_tag_text(key) {
        return Err(bad_request(format!("Invalid tag key '{}'", key)));
    }
    if key == DEPLOYMENT_ID_COST_TAG {
        return Err(bad_request(format!(
            "{} is filtered on with deployment_ids, not as a tag",
            DEPLOYMENT_ID_COST_TAG
        )));
    }
    Ok(key.to_string())
}

/// `tag_filters`: a JSON object of tag key to the values to keep, e.g.
/// `{"team":["platform","data"]}`. JSON rather than a delimited list, since tag keys and values
/// may contain `:`, `=` and spaces. Empty values are dropped, and a key left with none is an error.
fn parse_tag_filters(payload: &Value) -> Result<Vec<(String, Vec<String>)>> {
    let Some(raw) = param(payload, "tag_filters") else {
        return Ok(Vec::new());
    };
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&raw).map_err(|_| {
        bad_request(
            r#"tag_filters must be a JSON object of tag key to values, e.g. {"team":["platform"]}"#,
        )
    })?;
    if parsed.len() > MAX_TAG_FILTERS {
        return Err(bad_request(format!(
            "At most {} tags can be filtered on",
            MAX_TAG_FILTERS
        )));
    }

    let mut filters = Vec::new();
    for (key, values) in parsed {
        let key = parse_tag_key(&key)?;
        let values: Vec<&str> = match &values {
            Value::String(v) => vec![v.as_str()],
            Value::Array(items) => items.iter().filter_map(|v| v.as_str()).collect(),
            _ => Vec::new(),
        };
        let mut kept: Vec<String> = Vec::new();
        for value in values.into_iter().map(str::trim).filter(|v| !v.is_empty()) {
            if value.len() > MAX_TAG_VALUE_LEN || !is_tag_text(value) {
                return Err(bad_request(format!(
                    "Invalid value '{}' for tag '{}'",
                    value, key
                )));
            }
            if !kept.iter().any(|v| v == value) {
                kept.push(value.to_string());
            }
        }
        if kept.is_empty() {
            return Err(bad_request(format!("Tag filter '{}' has no values", key)));
        }
        if kept.len() > MAX_FILTER_VALUES {
            return Err(bad_request(format!(
                "At most {} values can be filtered on for tag '{}'",
                MAX_FILTER_VALUES, key
            )));
        }
        filters.push((key, kept));
    }
    filters.sort();
    Ok(filters)
}

fn parse_group_by(value: &str) -> Result<Vec<String>> {
    let mut group_by: Vec<String> = Vec::new();
    for dimension in value.split(',').map(str::trim).filter(|d| !d.is_empty()) {
        let dimension = dimension.to_ascii_uppercase();
        if !GROUP_BY_DIMENSIONS.contains(&dimension.as_str()) {
            return Err(bad_request(format!(
                "Invalid group_by dimension '{}'. Expected one of {}",
                dimension,
                GROUP_BY_DIMENSIONS.join(", ")
            )));
        }
        if !group_by.contains(&dimension) {
            group_by.push(dimension);
        }
    }
    if group_by.len() > MAX_GROUP_BY {
        return Err(bad_request(format!(
            "At most {} group_by dimensions are supported",
            MAX_GROUP_BY
        )));
    }
    Ok(group_by)
}

/// A non-empty string or number parameter.
fn param(payload: &Value, name: &str) -> Option<String> {
    match payload.get(name)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_time(value: &str, name: &str) -> Result<NaiveDateTime> {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Ok(date.and_hms_opt(0, 0, 0).unwrap_or_default());
    }
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.naive_utc())
        .map_err(|_| {
            bad_request(format!(
                "Invalid '{}' '{}'. Expected YYYY-MM-DD or an RFC 3339 time",
                name, value
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 14, 37, 12).unwrap()
    }

    fn parse(payload: Value) -> Result<CostRequest> {
        parse_cost_request(&payload, now())
    }

    #[test]
    fn defaults_to_the_last_30_days_by_service() {
        let request = parse(json!({})).unwrap();

        assert_eq!(
            request,
            CostRequest {
                granularity: CostGranularity::Daily,
                start: "2026-09-14".to_string(),
                end: "2026-10-14".to_string(),
                metric: "AmortizedCost".to_string(),
                group_by: vec!["SERVICE".to_string()],
            }
        );
    }

    #[test]
    fn monthly_periods_start_on_the_first_of_the_month() {
        let request = parse(json!({ "granularity": "monthly", "periods": "3" })).unwrap();

        assert_eq!(request.start, "2026-08-01");
        assert_eq!(request.end, "2026-11-01");
    }

    #[test]
    fn hourly_ranges_use_timestamps_on_the_hour() {
        let request = parse(json!({ "granularity": "HOURLY", "periods": 2 })).unwrap();

        assert_eq!(request.start, "2026-10-13T13:00:00Z");
        assert_eq!(request.end, "2026-10-13T15:00:00Z");
    }

    #[test]
    fn explicit_range_is_used_as_given() {
        let request = parse(json!({
            "granularity": "DAILY",
            "start": "2026-01-01",
            "end": "2026-02-01T00:00:00Z",
        }))
        .unwrap();

        assert_eq!(request.start, "2026-01-01");
        assert_eq!(request.end, "2026-02-01");
    }

    #[test]
    fn group_by_is_normalized_and_may_be_empty() {
        let request = parse(json!({ "group_by": "usage_type, region,usage_type" })).unwrap();
        assert_eq!(request.group_by, vec!["USAGE_TYPE", "REGION"]);

        let request = parse(json!({ "group_by": "" })).unwrap();
        assert!(request.group_by.is_empty());
    }

    #[test]
    fn metric_is_matched_case_insensitively() {
        let request = parse(json!({ "metric": "unblendedcost" })).unwrap();
        assert_eq!(request.metric, "UnblendedCost");
    }

    #[test]
    fn account_costs_take_one_dimension_and_filters() {
        let request = parse_account_cost_request(
            &json!({
                "granularity": "MONTHLY",
                "periods": "3",
                "group_by": "region",
                "services": "Amazon S3, AWS Lambda,,Amazon S3",
                "deployment_ids": "s3bucket/a,ec2/b",
            }),
            now(),
        )
        .unwrap();

        assert_eq!(request.range.start, "2026-08-01");
        assert_eq!(request.query_granularity, CostGranularity::Monthly);
        assert_eq!(
            request.group_by,
            Some(CostGroupBy::Dimension("REGION".to_string()))
        );
        assert_eq!(request.filters.services, vec!["Amazon S3", "AWS Lambda"]);
        assert_eq!(request.filters.deployment_ids, vec!["s3bucket/a", "ec2/b"]);
        assert!(request.filters.regions.is_empty());

        let request = parse_account_cost_request(&json!({}), now()).unwrap();
        assert_eq!(request.group_by, None);
        assert_eq!(request.query_granularity, CostGranularity::Daily);

        assert!(
            parse_account_cost_request(&json!({ "group_by": "SERVICE,REGION" }), now()).is_err()
        );
    }

    #[test]
    fn account_costs_can_be_grouped_and_filtered_by_tags() {
        let request = parse_account_cost_request(
            &json!({
                "group_by": "TAG:cost-center",
                "tag_filters": r#"{"team":["platform"," data ","platform"],"aws:createdBy":"x"}"#,
            }),
            now(),
        )
        .unwrap();

        assert_eq!(
            request.group_by,
            Some(CostGroupBy::Tag("cost-center".to_string()))
        );
        assert_eq!(
            request.filters.tags,
            vec![
                ("aws:createdBy".to_string(), vec!["x".to_string()]),
                (
                    "team".to_string(),
                    vec!["platform".to_string(), "data".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn tag_keys_are_listed_over_days() {
        assert_eq!(
            parse_tag_keys_request(&json!({}), now()).unwrap(),
            ("2026-07-16".to_string(), "2026-10-14".to_string())
        );
        assert_eq!(
            parse_tag_keys_request(&json!({ "days": "7" }), now())
                .unwrap()
                .0,
            "2026-10-07"
        );
        assert!(parse_tag_keys_request(&json!({ "days": "0" }), now()).is_err());
    }

    #[test]
    fn invalid_tags_are_rejected() {
        for payload in [
            json!({ "group_by": "tag:" }),
            json!({ "group_by": "tag:INFRAWEAVE_DEPLOYMENT_ID" }),
            json!({ "group_by": "tag:team,owner" }),
            json!({ "tag_filters": "team=platform" }),
            json!({ "tag_filters": r#"{"team":[]}"# }),
            json!({ "tag_filters": r#"{"team":["  "]}"# }),
            json!({ "tag_filters": r#"{"INFRAWEAVE_DEPLOYMENT_ID":["s3bucket/a"]}"# }),
            json!({ "tag_filters": r#"{"team":["a;b"]}"# }),
        ] {
            assert!(
                parse_account_cost_request(&payload, now()).is_err(),
                "accepted {}",
                payload
            );
        }
    }

    #[test]
    fn totals_only_asks_for_monthly_periods_over_the_same_range() {
        let request =
            parse_account_cost_request(&json!({ "periods": "7", "series": "false" }), now())
                .unwrap();

        assert_eq!(request.range.start, "2026-10-07");
        assert_eq!(request.range.end, "2026-10-14");
        assert_eq!(request.query_granularity, CostGranularity::Monthly);
    }

    #[test]
    fn empty_filter_entries_filter_on_nothing() {
        let request =
            parse_account_cost_request(&json!({ "deployment_ids": " , ," }), now()).unwrap();
        assert!(request.filters.deployment_ids.is_empty());
    }

    #[test]
    fn resource_costs_cover_the_last_days_and_need_a_filter() {
        let request = parse_resource_cost_request(
            &json!({ "days": "7", "deployment_ids": "s3bucket/a", "start": "2020-01-01" }),
            now(),
        )
        .unwrap();
        assert_eq!(request.start, "2026-10-07");
        assert_eq!(request.end, "2026-10-14");

        assert!(
            parse_resource_cost_request(&json!({ "days": "15", "services": "x" }), now()).is_err()
        );
        assert!(parse_resource_cost_request(&json!({}), now()).is_err());
        assert!(parse_resource_cost_request(&json!({ "deployment_ids": "," }), now()).is_err());
    }

    #[test]
    fn invalid_options_are_rejected() {
        for payload in [
            json!({ "granularity": "WEEKLY" }),
            json!({ "group_by": "SERVICE,REGION,USAGE_TYPE" }),
            json!({ "group_by": "TAG" }),
            json!({ "metric": "UsageQuantity" }),
            json!({ "periods": "0" }),
            json!({ "granularity": "HOURLY", "periods": "500" }),
            json!({ "start": "2026-10-10", "end": "2026-10-10" }),
            json!({ "start": "yesterday" }),
            json!({ "granularity": "HOURLY", "start": "2026-09-01", "end": "2026-10-01" }),
        ] {
            assert!(parse(payload.clone()).is_err(), "accepted {}", payload);
        }
    }
}
