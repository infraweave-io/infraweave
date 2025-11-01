"""
MkDocs plugin to automatically generate changelog from GitHub releases.
"""

import json
import os
from datetime import datetime
from urllib.request import urlopen, Request
from urllib.error import HTTPError

from mkdocs.plugins import BasePlugin
from mkdocs.config import config_options


class ChangelogPlugin(BasePlugin):
    """Generate changelog from GitHub releases."""
    
    config_scheme = (
        ('repo_owner', config_options.Type(str, default='infraweave-io')),
        ('repo_name', config_options.Type(str, default='infraweave')),
        ('include_prereleases', config_options.Type(bool, default=False)),
        ('output_file', config_options.Type(str, default='docs/changelog.md')),
    )
    
    def on_pre_build(self, config):
        """Generate changelog before building documentation."""
        print("Fetching releases from GitHub...")
        releases = self._fetch_releases()
        
        if releases:
            print(f"Found {len(releases)} releases.")
            changelog = self._generate_changelog(releases)
            
            # Write to output file
            output_path = self.config['output_file']
            with open(output_path, 'w') as f:
                f.write(changelog)
            print(f"✓ Changelog generated at {output_path}")
        else:
            print("No releases found or error occurred.")
    
    def _fetch_releases(self):
        """Fetch releases from GitHub API."""
        repo_owner = self.config['repo_owner']
        repo_name = self.config['repo_name']
        api_url = f"https://api.github.com/repos/{repo_owner}/{repo_name}/releases"
        
        headers = {"Accept": "application/vnd.github.v3+json"}
        
        # Add GitHub token if available (for higher rate limits)
        github_token = os.environ.get("GITHUB_TOKEN")
        if github_token:
            headers["Authorization"] = f"token {github_token}"
        
        try:
            req = Request(api_url, headers=headers)
            with urlopen(req) as response:
                releases = json.loads(response.read().decode())
                
            # Filter out pre-releases if configured
            if not self.config['include_prereleases']:
                releases = [r for r in releases if not r['prerelease']]
            
            return releases
        except HTTPError as e:
            print(f"Error fetching releases: {e}")
            return []
    
    def _format_date(self, date_str):
        """Format ISO date string to readable format."""
        dt = datetime.strptime(date_str, "%Y-%m-%dT%H:%M:%SZ")
        return dt.strftime("%B %d, %Y")
    
    def _convert_urls_to_links(self, text):
        """Convert plain GitHub URLs to markdown links."""
        import re
        
        # Pattern for GitHub PR/issue URLs
        pr_pattern = r'https://github\.com/([\w-]+)/([\w-]+)/(pull|issues)/(\d+)'
        
        def replace_pr_url(match):
            full_url = match.group(0)
            owner = match.group(1)
            repo = match.group(2)
            pr_type = match.group(3)
            number = match.group(4)
            # Create markdown link with PR/issue number as text
            return f'[#{number}]({full_url})'
        
        # Pattern for GitHub compare URLs (Full Changelog)
        compare_pattern = r'https://github\.com/([\w-]+)/([\w-]+)/compare/([\w.-]+)\.\.\.([\w.-]+)'
        
        def replace_compare_url(match):
            full_url = match.group(0)
            # Extract the comparison part (e.g., v0.0.93...v0.0.94)
            from_version = match.group(3)
            to_version = match.group(4)
            link_text = f'{from_version}...{to_version}'
            return f'[{link_text}]({full_url})'
        
        # Apply both patterns
        text = re.sub(pr_pattern, replace_pr_url, text)
        text = re.sub(compare_pattern, replace_compare_url, text)
        
        return text
    
    def _demote_headings(self, text):
        """Demote all markdown headings by one level (## becomes ###, etc)."""
        import re
        lines = []
        for line in text.split('\n'):
            # Match headings (##, ###, etc.) but not the first # (main title)
            if re.match(r'^#{2,}\s', line):
                # Add one more # to demote the heading
                lines.append('#' + line)
            else:
                lines.append(line)
        return '\n'.join(lines)
    
    def _generate_changelog(self, releases):
        """Generate changelog markdown from releases."""
        lines = [
            "---",
            "toc_depth: 2",
            "---",
            "",
            "# Changelog",
            "",
            "All notable changes to InfraWeave are documented here.",
            "",
            "This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).",
            "",
            "*This changelog is automatically generated from [GitHub releases](https://github.com/{}/{}/releases).*".format(
                self.config['repo_owner'], self.config['repo_name']
            ),
            "",
            "---",
            "",
        ]
        
        for release in releases:
            tag = release["tag_name"]
            name = release["name"] or tag
            date = self._format_date(release["published_at"])
            body = release["body"] or "No release notes provided."
            url = release["html_url"]
            is_prerelease = release["prerelease"]
            
            # Convert plain URLs to markdown links
            body = self._convert_urls_to_links(body)
            
            # Demote all headings in release body so only versions show in TOC
            body = self._demote_headings(body)
            
            # Add release header
            prerelease_tag = " (Pre-release)" if is_prerelease else ""
            lines.append(f"## [{name}]({url}){prerelease_tag}")
            lines.append(f"*Released on {date}*")
            lines.append("")
            
            # Add release body
            lines.append(body)
            lines.append("")
            lines.append("---")
            lines.append("")
        
        if not releases:
            lines.append("*No releases yet.*")
            lines.append("")
        
        return "\n".join(lines)
