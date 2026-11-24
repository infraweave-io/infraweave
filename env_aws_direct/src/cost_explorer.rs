// Deployment cost from Cost Explorer, read in the project's own account so a
// linked account only ever sees its own spend.
use anyhow::{anyhow, Result};
use aws_sdk_costexplorer::error::{DisplayErrorContext, ProvideErrorMetadata, SdkError};
use aws_sdk_costexplorer::types::{
    DateInterval, DimensionValues, Expression, Granularity, GroupDefinition, GroupDefinitionType,
    MatchOption, MetricValue, ResultByTime, TagValues,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::direct_impl::{assume_role_config, get_aws_config, get_env_var};

/// The only tag costs are looked up by: the one the runner puts on every resource of a deployment.
pub const DEPLOYMENT_ID_COST_TAG: &str = "INFRAWEAVE_DEPLOYMENT_ID";

/// A Cost Explorer query for the resources of one deployment. The caller validates the options;
/// Cost Explorer rejects anything else with [`CostQueryRejected`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DeploymentCostQuery {
    pub project_id: String,
    pub region: String,
    /// Matched exactly against [`DEPLOYMENT_ID_COST_TAG`]; never empty.
    pub deployment_id: String,
    /// `MONTHLY`, `DAILY` or `HOURLY`.
    pub granularity: String,
    /// Inclusive: `YYYY-MM-DD`, or `YYYY-MM-DDThh:mm:ssZ` for `HOURLY`.
    pub start: String,
    /// Exclusive, in the same format as `start`.
    pub end: String,
    /// `AmortizedCost`, `UnblendedCost`, ...
    pub metric: String,
    /// Up to two Cost Explorer dimensions (`SERVICE`, `USAGE_TYPE`, ...); empty for totals only.
    pub group_by: Vec<String>,
}

/// What an account-wide query is narrowed to. Each list is ORed within and the lists are ANDed;
/// an empty list does not filter. Values are never empty.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct CostFilters {
    /// Matched exactly against [`DEPLOYMENT_ID_COST_TAG`].
    pub deployment_ids: Vec<String>,
    /// Cost Explorer `SERVICE` values, e.g. `Amazon Simple Storage Service`.
    pub services: Vec<String>,
    /// Cost Explorer `REGION` values, e.g. `us-west-2` or `global`.
    pub regions: Vec<String>,
    /// Cost allocation tags and the values to narrow each to, sorted by key. Never the deployment
    /// id tag, which is what `deployment_ids` is for.
    pub tags: Vec<(String, Vec<String>)>,
    /// Modules, matched against the first segment of the deployment id (`module/name`). Cost
    /// Explorer cannot match the start of a tag value, so these are applied to its answer: unlike
    /// a list of ids, they also keep the deployments that have been destroyed since.
    pub modules: Vec<String>,
}

impl CostFilters {
    /// Whether nothing narrows the Cost Explorer query itself; `modules` only sorts its answer.
    pub fn is_empty(&self) -> bool {
        self.deployment_ids.is_empty()
            && self.services.is_empty()
            && self.regions.is_empty()
            && self.tags.is_empty()
    }

    fn keeps_deployment(&self, deployment_id: &str) -> bool {
        self.modules.is_empty() || {
            let module = deployment_id.split('/').next().unwrap_or_default();
            self.modules.iter().any(|m| m.eq_ignore_ascii_case(module))
        }
    }
}

/// What to break a deployment's cost down by, beyond the deployment itself.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CostGroupBy {
    /// A Cost Explorer dimension: `SERVICE`, `REGION`, ...
    Dimension(String),
    /// The values of a cost allocation tag. Costs without the tag fall in an empty value.
    Tag(String),
}

impl CostGroupBy {
    /// `SERVICE`, or `tag:<key>` for a tag, as the API takes it.
    pub fn name(&self) -> String {
        match self {
            Self::Dimension(dimension) => dimension.clone(),
            Self::Tag(key) => format!("tag:{}", key),
        }
    }
}

/// The cost of every deployment in one account, optionally broken down by one dimension.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AccountCostQuery {
    pub project_id: String,
    pub region: String,
    /// `MONTHLY`, `DAILY` or `HOURLY`.
    pub granularity: String,
    /// Inclusive: `YYYY-MM-DD`, or `YYYY-MM-DDThh:mm:ssZ` for `HOURLY`.
    pub start: String,
    /// Exclusive, in the same format as `start`.
    pub end: String,
    /// `AmortizedCost`, `UnblendedCost`, ...
    pub metric: String,
    /// What to break each deployment's cost down by.
    pub group_by: Option<CostGroupBy>,
    pub filters: CostFilters,
}

/// The cost of each resource of the deployments in one account, per day. Cost Explorer only
/// has this for the last 14 days, and only once resource-level data is enabled in its settings.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ResourceCostQuery {
    pub project_id: String,
    pub region: String,
    /// Inclusive `YYYY-MM-DD`, at most 14 days back.
    pub start: String,
    /// Exclusive `YYYY-MM-DD`.
    pub end: String,
    pub metric: String,
    /// Cost Explorer requires resource queries to be filtered, so this is never empty.
    pub filters: CostFilters,
}

/// The cost allocation tag keys with cost in one account over a range of days, as Cost Explorer
/// offers them to group by.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CostTagKeysQuery {
    pub project_id: String,
    pub region: String,
    /// Inclusive `YYYY-MM-DD`.
    pub start: String,
    /// Exclusive `YYYY-MM-DD`.
    pub end: String,
}

/// Cost Explorer refused the query itself (a range outside the retained data, hourly
/// granularity not enabled, ...) rather than failing to answer it.
#[derive(Debug)]
pub struct CostQueryRejected(pub String);

impl std::fmt::Display for CostQueryRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CostQueryRejected {}

/// Cost Explorer bills every request and refreshes its data only a few times a day, so an answer
/// is reused for hours instead of asking again on every page view.
pub const COST_CACHE_TTL: Duration = Duration::from_secs(5 * 60 * 60);
/// What the cache may hold in total, counted as the answers' serialized JSON. The web API Lambda
/// has 128MB, and one account's breakdown across its deployments can run to megabytes, so a
/// count of entries says nothing about the memory they take.
const COST_CACHE_MAX_BYTES: usize = 24 * 1024 * 1024;
/// A larger answer is not kept: it would push out many smaller ones for one reuse.
const COST_CACHE_MAX_ENTRY_BYTES: usize = COST_CACHE_MAX_BYTES / 4;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum CostCacheKey {
    Deployment(DeploymentCostQuery),
    Account(AccountCostQuery),
    Resources(ResourceCostQuery),
    TagKeys(CostTagKeysQuery),
}

/// Answers by question, kept as serialized JSON so their size is known exactly; a parsed `Value`
/// takes several times the memory of its text.
static COST_CACHE: LazyLock<RwLock<HashMap<CostCacheKey, (Instant, String)>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

async fn cached<F>(key: CostCacheKey, fetch: F) -> Result<Value>
where
    F: std::future::Future<Output = Result<Value>>,
{
    if let Some((fetched_at, cached)) = COST_CACHE.read().await.get(&key) {
        if fetched_at.elapsed() < COST_CACHE_TTL {
            if let Ok(value) = serde_json::from_str(cached) {
                return Ok(value);
            }
        }
    }

    let result = fetch.await?;
    let serialized = serde_json::to_string(&result)?;
    if serialized.len() <= COST_CACHE_MAX_ENTRY_BYTES {
        let mut cache = COST_CACHE.write().await;
        store(&mut cache, key, serialized, Instant::now());
    }
    Ok(result)
}

/// Adds an answer, first dropping expired ones and then the oldest until it fits the budget.
fn store(
    cache: &mut HashMap<CostCacheKey, (Instant, String)>,
    key: CostCacheKey,
    serialized: String,
    now: Instant,
) {
    cache.remove(&key);
    cache.retain(|_, (fetched_at, _)| now.duration_since(*fetched_at) < COST_CACHE_TTL);
    let mut used: usize = cache.values().map(|(_, json)| json.len()).sum();
    while used + serialized.len() > COST_CACHE_MAX_BYTES {
        let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (fetched_at, _))| *fetched_at)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        if let Some((_, json)) = cache.remove(&oldest) {
            used -= json.len();
        }
    }
    cache.insert(key, (now, serialized));
}

pub async fn get_deployment_cost_cross_account(query: &DeploymentCostQuery) -> Result<Value> {
    ensure_deployment_id(&query.deployment_id)?;
    cached(
        CostCacheKey::Deployment(query.clone()),
        fetch_deployment_cost(query),
    )
    .await
}

/// One Cost Explorer query for a whole account, grouped by the deployment id tag, so a list of
/// deployments costs one request per account rather than one per deployment. Costs of resources
/// without the tag are left out, whatever else the query is grouped or filtered by.
pub async fn get_account_costs_cross_account(query: &AccountCostQuery) -> Result<Value> {
    ensure_filter_values(&query.filters)?;
    if let Some(CostGroupBy::Tag(key)) = &query.group_by {
        ensure_other_tag(key)?;
    }
    cached(
        CostCacheKey::Account(query.clone()),
        fetch_account_costs(query),
    )
    .await
}

/// The cost of each resource of the deployments in an account, from Cost Explorer's
/// resource-level data. Like the account query, only tagged resources are returned.
pub async fn get_resource_costs_cross_account(query: &ResourceCostQuery) -> Result<Value> {
    ensure_filter_values(&query.filters)?;
    if query.filters.is_empty() {
        return Err(CostQueryRejected(
            "Resource costs need a deployment or service to be narrowed to".to_string(),
        )
        .into());
    }
    cached(
        CostCacheKey::Resources(query.clone()),
        fetch_resource_costs(query),
    )
    .await
}

/// The tag keys only, not their values: values would include those of resources outside any
/// deployment, so they are read from deployment costs grouped by the tag instead. The deployment
/// id tag itself is left out, as it cannot be grouped or filtered by as a tag.
pub async fn get_cost_tag_keys_cross_account(query: &CostTagKeysQuery) -> Result<Value> {
    cached(
        CostCacheKey::TagKeys(query.clone()),
        fetch_cost_tag_keys(query),
    )
    .await
}

async fn fetch_cost_tag_keys(query: &CostTagKeysQuery) -> Result<Value> {
    let client = cost_explorer_client(&query.project_id, &query.region).await?;
    let time_period = DateInterval::builder()
        .start(&query.start)
        .end(&query.end)
        .build()?;

    let mut keys: Vec<String> = Vec::new();
    let mut next_page_token: Option<String> = None;
    loop {
        let response = client
            .get_tags()
            .time_period(time_period.clone())
            .set_next_page_token(next_page_token.take())
            .send()
            .await
            .map_err(|e| cost_explorer_error("GetTags", e))?;
        keys.extend(response.tags().iter().cloned());
        match response.next_page_token() {
            Some(token) if !token.is_empty() => next_page_token = Some(token.to_string()),
            _ => break,
        }
    }

    Ok(json!({
        "project_id": query.project_id,
        "start": query.start,
        "end": query.end,
        "tag_keys": usable_tag_keys(keys),
    }))
}

fn usable_tag_keys(keys: Vec<String>) -> Vec<String> {
    let mut keys: Vec<String> = keys
        .into_iter()
        .filter(|k| !k.trim().is_empty() && k != DEPLOYMENT_ID_COST_TAG)
        .collect();
    keys.sort_by_key(|k| k.to_lowercase());
    keys.dedup();
    keys
}

/// Each filter value is matched exactly, and an empty one would match what has no value at all:
/// for the deployment id tag, every untagged resource.
fn ensure_filter_values(filters: &CostFilters) -> Result<()> {
    let tag_values = filters.tags.iter().flat_map(|(_, values)| values);
    let tag_keys = filters.tags.iter().map(|(key, _)| key);
    let values = filters
        .deployment_ids
        .iter()
        .chain(&filters.services)
        .chain(&filters.regions)
        .chain(&filters.modules)
        .chain(tag_keys)
        .chain(tag_values);
    for value in values {
        if value.trim().is_empty() {
            return Err(CostQueryRejected("Cost filters must not be empty".to_string()).into());
        }
    }
    for (key, values) in &filters.tags {
        ensure_other_tag(key)?;
        if values.is_empty() {
            return Err(CostQueryRejected(format!("Tag filter '{}' has no values", key)).into());
        }
    }
    Ok(())
}

/// The deployment id tag is grouped by and filtered on through `deployment_ids` only, so its
/// empty-value check cannot be bypassed.
fn ensure_other_tag(key: &str) -> Result<()> {
    if key.trim().is_empty() || key == DEPLOYMENT_ID_COST_TAG {
        return Err(CostQueryRejected(format!(
            "'{}' cannot be used as a tag here; filter on deployment ids instead",
            key
        ))
        .into());
    }
    Ok(())
}

fn tag_filter(key: &str, values: &[String]) -> Expression {
    Expression::builder()
        .tags(
            TagValues::builder()
                .key(key)
                .set_values(Some(values.to_vec()))
                .match_options(MatchOption::Equals)
                .build(),
        )
        .build()
}

/// The filters as one Cost Explorer expression, or `None` when there are none.
fn filter_expression(filters: &CostFilters) -> Option<Expression> {
    let mut expressions = Vec::new();
    if !filters.deployment_ids.is_empty() {
        expressions.push(tag_filter(DEPLOYMENT_ID_COST_TAG, &filters.deployment_ids));
    }
    for (key, values) in &filters.tags {
        expressions.push(tag_filter(key, values));
    }
    for (dimension, values) in [("SERVICE", &filters.services), ("REGION", &filters.regions)] {
        if !values.is_empty() {
            expressions.push(
                Expression::builder()
                    .dimensions(
                        DimensionValues::builder()
                            .key(dimension.into())
                            .set_values(Some(values.clone()))
                            .build(),
                    )
                    .build(),
            );
        }
    }
    // Cost Explorer wants at least two expressions under an AND.
    match expressions.len() {
        0 => None,
        1 => expressions.pop(),
        _ => Some(Expression::builder().set_and(Some(expressions)).build()),
    }
}

/// Cost Explorer reads an empty tag value as "untagged", which would return everything in the
/// account that is not part of any deployment.
fn ensure_deployment_id(deployment_id: &str) -> Result<()> {
    if deployment_id.trim().is_empty() {
        return Err(
            CostQueryRejected("A deployment id is required to look up cost".to_string()).into(),
        );
    }
    Ok(())
}

/// Cost Explorer as seen from `project_id`'s account.
async fn cost_explorer_client(
    project_id: &str,
    region: &str,
) -> Result<aws_sdk_costexplorer::Client> {
    let central_account_id = get_env_var("CENTRAL_ACCOUNT_ID")?;

    let config = if central_account_id == project_id {
        get_aws_config(None).await
    } else {
        // The log-read role, under the session name read_logs uses, so both share one assumed role.
        assume_role_config(
            project_id,
            "infraweave_api_read_log",
            "CentralApiAssumeRoleSession",
            region,
        )
        .await?
    };
    // Cost Explorer has one endpoint per partition, whichever region the deployment is in.
    let ce_config = aws_sdk_costexplorer::config::Builder::from(&config)
        .region(aws_config::Region::new(cost_explorer_region(region)))
        .build();
    Ok(aws_sdk_costexplorer::Client::from_conf(ce_config))
}

async fn fetch_deployment_cost(query: &DeploymentCostQuery) -> Result<Value> {
    let client = cost_explorer_client(&query.project_id, &query.region).await?;

    let filter = Expression::builder()
        .tags(
            TagValues::builder()
                .key(DEPLOYMENT_ID_COST_TAG)
                .values(&query.deployment_id)
                .match_options(MatchOption::Equals)
                .build(),
        )
        .build();
    let group_by = query
        .group_by
        .iter()
        .map(|dimension| {
            GroupDefinition::builder()
                .r#type(GroupDefinitionType::Dimension)
                .key(dimension)
                .build()
        })
        .collect();

    let (periods, unit) = query_cost_and_usage(
        &client,
        &query.start,
        &query.end,
        Granularity::from(query.granularity.as_str()),
        &query.metric,
        Some(filter),
        group_by,
    )
    .await?;

    Ok(summarize(query, periods, unit))
}

fn tag_group(key: &str) -> GroupDefinition {
    GroupDefinition::builder()
        .r#type(GroupDefinitionType::Tag)
        .key(key)
        .build()
}

fn deployment_tag_group() -> GroupDefinition {
    tag_group(DEPLOYMENT_ID_COST_TAG)
}

fn dimension_group(dimension: &str) -> GroupDefinition {
    GroupDefinition::builder()
        .r#type(GroupDefinitionType::Dimension)
        .key(dimension)
        .build()
}

async fn fetch_account_costs(query: &AccountCostQuery) -> Result<Value> {
    let client = cost_explorer_client(&query.project_id, &query.region).await?;

    // Grouped by the tag first, whatever else is asked, so untagged costs can be told apart and
    // dropped: Cost Explorer cannot filter on "has the tag" without naming the values.
    let mut group_by = vec![deployment_tag_group()];
    match &query.group_by {
        Some(CostGroupBy::Dimension(dimension)) => group_by.push(dimension_group(dimension)),
        Some(CostGroupBy::Tag(key)) => group_by.push(tag_group(key)),
        None => {}
    }

    let (periods, unit) = query_cost_and_usage(
        &client,
        &query.start,
        &query.end,
        Granularity::from(query.granularity.as_str()),
        &query.metric,
        filter_expression(&query.filters),
        group_by,
    )
    .await?;

    Ok(summarize_account(query, periods, unit))
}

async fn fetch_resource_costs(query: &ResourceCostQuery) -> Result<Value> {
    let client = cost_explorer_client(&query.project_id, &query.region).await?;
    let time_period = DateInterval::builder()
        .start(&query.start)
        .end(&query.end)
        .build()?;

    let mut periods: BTreeMap<String, Period> = BTreeMap::new();
    let mut unit: Option<String> = None;
    let mut next_page_token: Option<String> = None;

    loop {
        let response = client
            .get_cost_and_usage_with_resources()
            .time_period(time_period.clone())
            .granularity(Granularity::Daily)
            .metrics(&query.metric)
            .set_filter(filter_expression(&query.filters))
            .group_by(deployment_tag_group())
            .group_by(dimension_group("RESOURCE_ID"))
            .set_next_page_token(next_page_token.take())
            .send()
            .await
            .map_err(|e| cost_explorer_error("GetCostAndUsageWithResources", e))?;

        merge_results(
            &mut periods,
            &mut unit,
            response.results_by_time(),
            &query.metric,
        );

        match response.next_page_token() {
            Some(token) if !token.is_empty() => next_page_token = Some(token.to_string()),
            _ => break,
        }
    }

    let periods: Vec<Period> = periods.into_values().collect();
    let estimated = periods.iter().any(|p| p.estimated);
    let mut resources: HashMap<(String, String), f64> = HashMap::new();
    for (keys, amount) in periods.into_iter().flat_map(|p| p.groups) {
        if let (Some(deployment_id), Some(resource_id)) = (
            deployment_tag_value(&keys).filter(|id| query.filters.keeps_deployment(id)),
            keys.get(1),
        ) {
            *resources
                .entry((deployment_id.to_string(), resource_id.clone()))
                .or_default() += amount;
        }
    }
    let mut resources: Vec<((String, String), f64)> = resources.into_iter().collect();
    resources.sort_by(|(a_key, a), (b_key, b)| b.total_cmp(a).then_with(|| a_key.cmp(b_key)));
    let total: f64 = resources.iter().map(|(_, amount)| amount).sum();

    Ok(json!({
        "tag_key": DEPLOYMENT_ID_COST_TAG,
        "project_id": query.project_id,
        "start": query.start,
        "end": query.end,
        "metric": query.metric,
        "unit": unit.unwrap_or_else(|| "USD".to_string()),
        "estimated": estimated,
        "total": total,
        "resources": resources
            .into_iter()
            .map(|((deployment_id, resource_id), amount)| json!({
                "deployment_id": deployment_id,
                "resource_id": resource_id,
                "amount": amount,
            }))
            .collect::<Vec<_>>(),
    }))
}

/// Every page of a GetCostAndUsage query, merged into one entry per period.
async fn query_cost_and_usage(
    client: &aws_sdk_costexplorer::Client,
    start: &str,
    end: &str,
    granularity: Granularity,
    metric: &str,
    filter: Option<Expression>,
    group_by: Vec<GroupDefinition>,
) -> Result<(Vec<Period>, Option<String>)> {
    let time_period = DateInterval::builder().start(start).end(end).build()?;

    let mut periods: BTreeMap<String, Period> = BTreeMap::new();
    let mut unit: Option<String> = None;
    let mut next_page_token: Option<String> = None;

    loop {
        let response = client
            .get_cost_and_usage()
            .time_period(time_period.clone())
            .granularity(granularity.clone())
            .metrics(metric)
            .set_filter(filter.clone())
            .set_group_by(Some(group_by.clone()))
            .set_next_page_token(next_page_token.take())
            .send()
            .await
            .map_err(|e| cost_explorer_error("GetCostAndUsage", e))?;

        merge_results(&mut periods, &mut unit, response.results_by_time(), metric);

        match response.next_page_token() {
            Some(token) if !token.is_empty() => next_page_token = Some(token.to_string()),
            _ => break,
        }
    }

    Ok((periods.into_values().collect(), unit))
}

/// Adds one page of results. Pages split the groups, not the periods, so a period can arrive in
/// several pages.
fn merge_results(
    periods: &mut BTreeMap<String, Period>,
    unit: &mut Option<String>,
    results: &[ResultByTime],
    metric: &str,
) {
    for result in results {
        let (start, end) = result
            .time_period()
            .map(|p| (p.start().to_string(), p.end().to_string()))
            .unwrap_or_default();
        let period = periods.entry(start.clone()).or_insert_with(|| Period {
            start,
            end,
            ..Default::default()
        });
        period.estimated |= result.estimated();

        if let Some(value) = result.total().and_then(|t| t.get(metric)) {
            note_unit(unit, value);
            period.total = Some(period.total.unwrap_or(0.0) + amount(value));
        }
        for group in result.groups() {
            if let Some(value) = group.metrics().and_then(|m| m.get(metric)) {
                note_unit(unit, value);
                period.groups.push((group.keys().to_vec(), amount(value)));
            }
        }
    }
}

/// The deployment id of a group whose first key is the deployment id tag. Tag groups come back
/// keyed `KEY$value`, and resources without the tag as `KEY$`: those are not a deployment.
fn deployment_tag_value(keys: &[String]) -> Option<&str> {
    let value = tag_value(keys.first()?, DEPLOYMENT_ID_COST_TAG)?;
    (!value.trim().is_empty()).then_some(value)
}

/// The value of a `KEY$value` tag group key; empty for costs without the tag.
fn tag_value<'a>(group_key: &'a str, tag_key: &str) -> Option<&'a str> {
    group_key.strip_prefix(tag_key)?.strip_prefix('$')
}

/// Totals per deployment and, when broken down, per dimension value and per pair of the two,
/// over the whole range and per period. Untagged costs are dropped before anything is summed.
fn summarize_account(
    query: &AccountCostQuery,
    periods: Vec<Period>,
    unit: Option<String>,
) -> Value {
    let grouped = query.group_by.is_some();
    let mut deployments: HashMap<String, f64> = HashMap::new();
    let mut groups: HashMap<String, f64> = HashMap::new();
    let mut breakdown: HashMap<(String, String), f64> = HashMap::new();
    let mut total = 0.0;
    let mut estimated = false;

    let periods: Vec<Value> = periods
        .into_iter()
        .map(|period| {
            estimated |= period.estimated;
            let mut period_deployments: HashMap<String, f64> = HashMap::new();
            let mut period_groups: HashMap<String, f64> = HashMap::new();
            let mut period_total = 0.0;

            for (keys, amount) in &period.groups {
                let Some(deployment_id) = deployment_tag_value(keys) else {
                    continue;
                };
                if !query.filters.keeps_deployment(deployment_id) {
                    continue;
                }
                period_total += amount;
                *period_deployments
                    .entry(deployment_id.to_string())
                    .or_default() += amount;
                *deployments.entry(deployment_id.to_string()).or_default() += amount;
                if let Some(group_by) = &query.group_by {
                    let raw = keys.get(1).map(String::as_str).unwrap_or_default();
                    let key = match group_by {
                        CostGroupBy::Tag(tag_key) => tag_value(raw, tag_key).unwrap_or(raw),
                        CostGroupBy::Dimension(_) => raw,
                    }
                    .to_string();
                    *period_groups.entry(key.clone()).or_default() += amount;
                    *groups.entry(key.clone()).or_default() += amount;
                    *breakdown
                        .entry((deployment_id.to_string(), key))
                        .or_default() += amount;
                }
            }
            total += period_total;

            let mut value = json!({
                "start": period.start,
                "end": period.end,
                "estimated": period.estimated,
                "total": period_total,
                "deployments": amounts_json("deployment_id", period_deployments),
            });
            if grouped {
                value["groups"] = json!(amounts_json("key", period_groups));
            }
            value
        })
        .collect();

    let mut result = json!({
        "tag_key": DEPLOYMENT_ID_COST_TAG,
        "project_id": query.project_id,
        "granularity": query.granularity,
        "start": query.start,
        "end": query.end,
        "metric": query.metric,
        "group_by": query.group_by.as_ref().map(CostGroupBy::name),
        "unit": unit.unwrap_or_else(|| "USD".to_string()),
        "estimated": estimated,
        "total": total,
        "deployments": amounts_json("deployment_id", deployments),
        "periods": periods,
    });
    if grouped {
        result["groups"] = json!(amounts_json("key", groups));
        let mut breakdown: Vec<((String, String), f64)> = breakdown.into_iter().collect();
        breakdown.sort_by(|(a_key, a), (b_key, b)| b.total_cmp(a).then_with(|| a_key.cmp(b_key)));
        result["breakdown"] = json!(breakdown
            .into_iter()
            .map(|((deployment_id, key), amount)| json!({
                "deployment_id": deployment_id,
                "key": key,
                "amount": amount,
            }))
            .collect::<Vec<_>>());
    }
    result
}

/// `[{ <name>: key, "amount": amount }]`, largest first.
fn amounts_json(name: &str, amounts: HashMap<String, f64>) -> Vec<Value> {
    let mut amounts: Vec<(String, f64)> = amounts.into_iter().collect();
    amounts.sort_by(|(a_key, a), (b_key, b)| b.total_cmp(a).then_with(|| a_key.cmp(b_key)));
    amounts
        .into_iter()
        .map(|(key, amount)| json!({ name: key, "amount": amount }))
        .collect()
}

#[derive(Default)]
struct Period {
    start: String,
    end: String,
    estimated: bool,
    /// What Cost Explorer reports as the total, which it only fills in for ungrouped results.
    total: Option<f64>,
    groups: Vec<(Vec<String>, f64)>,
}

fn summarize(query: &DeploymentCostQuery, periods: Vec<Period>, unit: Option<String>) -> Value {
    let mut group_totals: HashMap<Vec<String>, f64> = HashMap::new();
    let mut total = 0.0;

    let periods: Vec<Value> = periods
        .into_iter()
        .map(|mut period| {
            let period_total = if query.group_by.is_empty() {
                period.total.unwrap_or(0.0)
            } else {
                period.groups.iter().map(|(_, amount)| amount).sum()
            };
            total += period_total;
            for (keys, amount) in &period.groups {
                *group_totals.entry(keys.clone()).or_default() += amount;
            }
            sort_by_amount(&mut period.groups);
            json!({
                "start": period.start,
                "end": period.end,
                "estimated": period.estimated,
                "total": period_total,
                "groups": groups_json(&period.groups),
            })
        })
        .collect();

    let mut groups: Vec<(Vec<String>, f64)> = group_totals.into_iter().collect();
    sort_by_amount(&mut groups);

    json!({
        "tag_key": DEPLOYMENT_ID_COST_TAG,
        "tag_value": query.deployment_id,
        "granularity": query.granularity,
        "metric": query.metric,
        "group_by": query.group_by,
        "start": query.start,
        "end": query.end,
        "unit": unit.unwrap_or_else(|| "USD".to_string()),
        "total": total,
        "groups": groups_json(&groups),
        "periods": periods,
    })
}

fn sort_by_amount(groups: &mut [(Vec<String>, f64)]) {
    groups.sort_by(|(a_keys, a), (b_keys, b)| b.total_cmp(a).then_with(|| a_keys.cmp(b_keys)));
}

fn groups_json(groups: &[(Vec<String>, f64)]) -> Vec<Value> {
    groups
        .iter()
        .map(|(keys, amount)| json!({ "keys": keys, "amount": amount }))
        .collect()
}

fn amount(value: &MetricValue) -> f64 {
    value
        .amount()
        .and_then(|a| a.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn note_unit(unit: &mut Option<String>, value: &MetricValue) {
    if unit.is_none() {
        *unit = value.unit().map(str::to_string);
    }
}

fn cost_explorer_region(region: &str) -> &'static str {
    if region.starts_with("cn-") {
        "cn-northwest-1"
    } else if region.starts_with("us-gov-") {
        "us-gov-west-1"
    } else {
        "us-east-1"
    }
}

fn cost_explorer_error<E, R>(operation: &str, e: SdkError<E, R>) -> anyhow::Error
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
    R: std::fmt::Debug,
{
    if let SdkError::ServiceError(service) = &e {
        let err = service.err();
        let rejected = matches!(
            err.code(),
            Some(
                "BillExpirationException"
                    | "DataUnavailableException"
                    | "RequestChangedException"
                    | "ValidationException"
            )
        );
        if rejected {
            return CostQueryRejected(format!(
                "Cost Explorer rejected the query: {}",
                err.message()
                    .unwrap_or(err.code().unwrap_or("unknown error"))
            ))
            .into();
        }
    }
    anyhow!(
        "Cost Explorer {} failed: {}",
        operation,
        DisplayErrorContext(&e)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(group_by: &[&str]) -> DeploymentCostQuery {
        DeploymentCostQuery {
            project_id: "123456789012".to_string(),
            region: "us-west-2".to_string(),
            deployment_id: "s3bucket/bucket1".to_string(),
            granularity: "DAILY".to_string(),
            start: "2026-10-01".to_string(),
            end: "2026-10-03".to_string(),
            metric: "UnblendedCost".to_string(),
            group_by: group_by.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn keys(k: &str) -> Vec<String> {
        vec![k.to_string()]
    }

    #[tokio::test]
    async fn an_empty_deployment_id_is_rejected_before_cost_explorer_is_asked() {
        for deployment_id in ["", "   "] {
            let err = get_deployment_cost_cross_account(&DeploymentCostQuery {
                deployment_id: deployment_id.to_string(),
                ..query(&["SERVICE"])
            })
            .await
            .unwrap_err();
            assert!(err.downcast_ref::<CostQueryRejected>().is_some(), "{err}");
        }
    }

    #[test]
    fn grouped_periods_are_totalled_from_their_groups() {
        let periods = vec![
            Period {
                start: "2026-10-01".to_string(),
                end: "2026-10-02".to_string(),
                groups: vec![(keys("Amazon S3"), 1.5), (keys("AWS KMS"), 2.0)],
                ..Default::default()
            },
            Period {
                start: "2026-10-02".to_string(),
                end: "2026-10-03".to_string(),
                estimated: true,
                groups: vec![(keys("Amazon S3"), 1.0)],
                ..Default::default()
            },
        ];

        let summary = summarize(&query(&["SERVICE"]), periods, Some("USD".to_string()));

        assert_eq!(summary["total"], json!(4.5));
        assert_eq!(summary["periods"][0]["total"], json!(3.5));
        assert_eq!(
            summary["periods"][0]["groups"][0]["keys"],
            json!(["AWS KMS"])
        );
        assert_eq!(summary["periods"][1]["estimated"], json!(true));
        assert_eq!(
            summary["groups"],
            json!([
                { "keys": ["Amazon S3"], "amount": 2.5 },
                { "keys": ["AWS KMS"], "amount": 2.0 },
            ])
        );
    }

    #[test]
    fn ungrouped_periods_keep_the_reported_total() {
        let periods = vec![Period {
            start: "2026-10-01".to_string(),
            end: "2026-10-02".to_string(),
            total: Some(7.25),
            ..Default::default()
        }];

        let summary = summarize(&query(&[]), periods, None);

        assert_eq!(summary["total"], json!(7.25));
        assert_eq!(summary["groups"], json!([]));
        assert_eq!(summary["unit"], json!("USD"));
    }

    fn account_query(group_by: Option<&str>) -> AccountCostQuery {
        AccountCostQuery {
            project_id: "123456789012".to_string(),
            region: "us-west-2".to_string(),
            granularity: "DAILY".to_string(),
            start: "2026-10-01".to_string(),
            end: "2026-10-03".to_string(),
            metric: "UnblendedCost".to_string(),
            group_by: group_by.map(|g| match g.strip_prefix("tag:") {
                Some(key) => CostGroupBy::Tag(key.to_string()),
                None => CostGroupBy::Dimension(g.to_string()),
            }),
            filters: CostFilters::default(),
        }
    }

    fn tagged(deployment_id: &str, rest: &[&str]) -> Vec<String> {
        std::iter::once(format!("INFRAWEAVE_DEPLOYMENT_ID${}", deployment_id))
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn account_costs_are_summed_per_deployment_and_skip_untagged_resources() {
        let periods = vec![
            Period {
                start: "2026-10-01".to_string(),
                groups: vec![
                    (tagged("s3bucket/bucket1", &[]), 1.0),
                    (tagged("", &[]), 500.0),
                    (tagged("ec2/web", &[]), 4.0),
                ],
                ..Default::default()
            },
            Period {
                start: "2026-10-02".to_string(),
                groups: vec![
                    (tagged("s3bucket/bucket1", &[]), 2.5),
                    (vec!["unexpected".to_string()], 9.0),
                ],
                ..Default::default()
            },
        ];

        let summary = summarize_account(&account_query(None), periods, None);

        assert_eq!(summary["total"], json!(7.5));
        assert_eq!(
            summary["deployments"],
            json!([
                { "deployment_id": "ec2/web", "amount": 4.0 },
                { "deployment_id": "s3bucket/bucket1", "amount": 3.5 },
            ])
        );
        assert_eq!(summary["periods"][0]["total"], json!(5.0));
        assert!(summary.get("groups").is_none());
    }

    #[test]
    fn account_costs_broken_down_by_a_dimension_only_count_tagged_costs() {
        let periods = vec![Period {
            start: "2026-10-01".to_string(),
            groups: vec![
                (tagged("s3bucket/bucket1", &["Amazon S3"]), 1.0),
                (tagged("ec2/web", &["Amazon S3"]), 2.0),
                (tagged("ec2/web", &["Amazon EC2"]), 5.0),
                (tagged("", &["Amazon EC2"]), 100.0),
            ],
            ..Default::default()
        }];

        let summary = summarize_account(&account_query(Some("SERVICE")), periods, None);

        assert_eq!(summary["total"], json!(8.0));
        assert_eq!(
            summary["groups"],
            json!([
                { "key": "Amazon EC2", "amount": 5.0 },
                { "key": "Amazon S3", "amount": 3.0 },
            ])
        );
        assert_eq!(
            summary["breakdown"][0],
            json!({ "deployment_id": "ec2/web", "key": "Amazon EC2", "amount": 5.0 })
        );
        assert_eq!(
            summary["periods"][0]["groups"][0]["key"],
            json!("Amazon EC2")
        );
    }

    #[test]
    fn account_costs_broken_down_by_a_tag_use_its_values() {
        let periods = vec![Period {
            start: "2026-10-01".to_string(),
            groups: vec![
                (tagged("ec2/web", &["team$platform"]), 5.0),
                (tagged("s3bucket/a", &["team$"]), 1.0),
                (tagged("", &["team$platform"]), 100.0),
            ],
            ..Default::default()
        }];

        let summary = summarize_account(&account_query(Some("tag:team")), periods, None);

        assert_eq!(summary["group_by"], json!("tag:team"));
        assert_eq!(
            summary["groups"],
            json!([
                { "key": "platform", "amount": 5.0 },
                { "key": "", "amount": 1.0 },
            ])
        );
    }

    #[tokio::test]
    async fn the_deployment_tag_cannot_be_used_as_another_tag() {
        let err =
            get_account_costs_cross_account(&account_query(Some("tag:INFRAWEAVE_DEPLOYMENT_ID")))
                .await
                .unwrap_err();
        assert!(err.downcast_ref::<CostQueryRejected>().is_some(), "{err}");

        let err = get_account_costs_cross_account(&AccountCostQuery {
            filters: CostFilters {
                tags: vec![(
                    "INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                    vec!["x".to_string()],
                )],
                ..Default::default()
            },
            ..account_query(None)
        })
        .await
        .unwrap_err();
        assert!(err.downcast_ref::<CostQueryRejected>().is_some(), "{err}");

        let err = get_account_costs_cross_account(&AccountCostQuery {
            filters: CostFilters {
                tags: vec![("team".to_string(), vec!["".to_string()])],
                ..Default::default()
            },
            ..account_query(None)
        })
        .await
        .unwrap_err();
        assert!(err.downcast_ref::<CostQueryRejected>().is_some(), "{err}");
    }

    #[test]
    fn the_cache_drops_its_oldest_answers_to_stay_within_its_budget() {
        let key = |n: u32| {
            CostCacheKey::TagKeys(CostTagKeysQuery {
                project_id: n.to_string(),
                region: "us-west-2".to_string(),
                start: "2026-10-01".to_string(),
                end: "2026-10-02".to_string(),
            })
        };
        let start = Instant::now();
        let mut cache = HashMap::new();
        let third = "x".repeat(COST_CACHE_MAX_BYTES / 3);

        for n in 0..4 {
            store(
                &mut cache,
                key(n),
                third.clone(),
                start + Duration::from_secs(n as u64),
            );
        }

        assert_eq!(cache.len(), 3);
        assert!(!cache.contains_key(&key(0)), "the oldest answer goes first");
        assert!(cache.contains_key(&key(3)));
        let used: usize = cache.values().map(|(_, json)| json.len()).sum();
        assert!(used <= COST_CACHE_MAX_BYTES);

        // Expired answers go before any live one does.
        store(
            &mut cache,
            key(9),
            "y".to_string(),
            start + COST_CACHE_TTL + Duration::from_secs(2),
        );
        assert!(!cache.contains_key(&key(1)));
        assert!(cache.contains_key(&key(3)));
    }

    #[test]
    fn tag_keys_leave_out_the_deployment_tag() {
        let keys = vec![
            "owner".to_string(),
            "INFRAWEAVE_DEPLOYMENT_ID".to_string(),
            "Environment".to_string(),
            "".to_string(),
            "aws:createdBy".to_string(),
        ];
        assert_eq!(
            usable_tag_keys(keys),
            vec!["aws:createdBy", "Environment", "owner"]
        );
    }

    #[test]
    fn a_module_filter_keeps_its_deployments_whether_or_not_they_still_exist() {
        let periods = vec![Period {
            start: "2026-10-01".to_string(),
            groups: vec![
                (tagged("acustomer/live", &[]), 1.0),
                (tagged("acustomer/destroyed-last-week", &[]), 2.0),
                (tagged("acustomer/old-casing", &[]), 4.0),
                (tagged("acustomerx/other-module", &[]), 8.0),
                (tagged("vpc/main", &[]), 16.0),
            ],
            ..Default::default()
        }];
        let query = AccountCostQuery {
            filters: CostFilters {
                modules: vec!["acustomer".to_string()],
                ..Default::default()
            },
            ..account_query(None)
        };

        let summary = summarize_account(&query, periods, None);

        assert_eq!(summary["total"], json!(7.0));
        assert_eq!(summary["deployments"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn filters_become_one_expression() {
        assert!(filter_expression(&CostFilters::default()).is_none());

        let one = filter_expression(&CostFilters {
            services: vec!["Amazon S3".to_string()],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(one.dimensions().unwrap().values(), ["Amazon S3"]);

        let all = filter_expression(&CostFilters {
            deployment_ids: vec!["ec2/web".to_string()],
            regions: vec!["us-west-2".to_string()],
            tags: vec![("team".to_string(), vec!["platform".to_string()])],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(all.and().len(), 3);
        assert_eq!(all.and()[0].tags().unwrap().values(), ["ec2/web"]);
        assert_eq!(all.and()[1].tags().unwrap().key(), Some("team"));
    }

    #[tokio::test]
    async fn empty_filter_values_and_unfiltered_resource_queries_are_rejected() {
        let err = get_account_costs_cross_account(&AccountCostQuery {
            filters: CostFilters {
                deployment_ids: vec!["".to_string()],
                ..Default::default()
            },
            ..account_query(None)
        })
        .await
        .unwrap_err();
        assert!(err.downcast_ref::<CostQueryRejected>().is_some(), "{err}");

        let err = get_resource_costs_cross_account(&ResourceCostQuery {
            project_id: "123456789012".to_string(),
            region: "us-west-2".to_string(),
            start: "2026-10-01".to_string(),
            end: "2026-10-03".to_string(),
            metric: "UnblendedCost".to_string(),
            filters: CostFilters::default(),
        })
        .await
        .unwrap_err();
        assert!(err.downcast_ref::<CostQueryRejected>().is_some(), "{err}");
    }

    #[test]
    fn cost_explorer_is_called_in_the_partition_of_the_deployment() {
        assert_eq!(cost_explorer_region("eu-north-1"), "us-east-1");
        assert_eq!(cost_explorer_region("us-gov-east-1"), "us-gov-west-1");
        assert_eq!(cost_explorer_region("cn-north-1"), "cn-northwest-1");
    }
}
