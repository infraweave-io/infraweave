# Output message.rendered in a code block, stripping all ANSI codes.
# Usage: jq -rn -f jq-rendered-only.jq output.txt
# Or:   cat output.txt | jq -rn -f jq-rendered-only.jq
# Input: cargo/rustc JSON with --message-format=json-diagnostic-rendered-ansi

def strip_ansi:
  # CSI: ESC [ parameters final_letter (e.g. m for SGR, H, J, K for cursor)
  gsub("\u001b\\[[0-9;]*[a-zA-Z]"; "");

def info_visit_url:
  def to_md_link:
    (capture("for further information visit (?<url>.+)") | "for further information visit [\(.url)](\(.url))") // .;
  [.message.children[]? | select(
    .level == "help" and
    ((.spans // [] | length) == 0) and
    (.message | startswith("for further information visit"))
  ) | (.message | to_md_link)][0];

inputs
| select(.reason == "compiler-message" and .message.level == "error")
| (.message.rendered // "" | strip_ansi | gsub("\n+$"; "")) as $text
| (info_visit_url) as $info
| "```\n\($text)\n```"
+ (if $info then "\n\($info)" else "" end)
