#!/usr/bin/env python3
"""Fix: Delete garbled comments and re-send correct UTF-8 comments via Linear API."""

import json
import re
import time
import urllib.request
import urllib.error

AUTH = "os.environ["LINEAR_API_KEY"]"
PROJECT_ID = "72e576a7-e894-4e22-b52c-5d47d43a5912"
API = "https://api.linear.app/graphql"

import os
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
COMMENTS_FILE = os.path.join(SCRIPT_DIR, "linear-comments.json")

def linear_post(payload: dict) -> dict:
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        API,
        data=data,
        headers={
            "Authorization": AUTH,
            "Content-Type": "application/json; charset=utf-8",
        },
        method="POST",
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode("utf-8"))

def get_comment(title: str, comments: dict) -> str:
    rules = [
        (r"^RD-M4-0(19|2[0-9]|3[0-9])", "workspace_domain"),
        (r"^RD-M4-0(3[1-9]|4[0-6])", "file_watcher"),
        (r"^QA-M4-", "qa_m4"),
        (r"^GATE-M4", "gate_m4"),
        (r"^RD-M5-00[1-9]|^RD-M5-01[0-4]", "ui_infra"),
        (r"^RD-M5-01[5-9]|^RD-M5-02[0-9]|^RD-M5-030$", "task_tree"),
        (r"^RD-M5-03[1-9]|^RD-M5-040$", "inspector"),
        (r"^RD-M5-04[1-9]|^RD-M5-05[0-2]", "keyboard"),
        (r"^RD-M5-05[3-9]|^RD-M5-06[0-4]", "filter"),
        (r"^QA-M5-", "qa_m5"),
        (r"^GATE-M5", "gate_m5"),
    ]
    for pattern, key in rules:
        if re.search(pattern, title):
            return comments[key]
    return comments["ui_infra"]

def is_garbled(body: str) -> bool:
    """Detect if a comment body is garbled (from encoding failure)."""
    lines = body.split("\n")
    first_line = lines[0] if lines else ""

    # Pattern 1: ?? question marks (first sync failure)
    if re.match(r"^## \?\? \?\?\?\?\?\?", first_line):
        return True

    # Pattern 2: Mojibake - has our section structure but garbled H2 header
    has_sections = any(s in body for s in ["实现方案", "遇到的问题", "解决方案"])
    if has_sections:
        # Valid headers: "## 📋 进度更新" or "##  进度更新"
        valid_header = re.match(r"^## [📋]?\s*进度更新", first_line)
        if not valid_header:
            return True

    return False

def main():
    # Load comments from UTF-8 JSON file
    with open(COMMENTS_FILE, "r", encoding="utf-8") as f:
        comments = json.load(f)

    # Step 1: Fetch all Done issues
    print("=== Fetching Done issues ===")
    all_issues = []
    after = None

    while True:
        cursor_part = f', after: "{after}"' if after else ""
        resp = linear_post({
            "query": f'query {{ project(id: "{PROJECT_ID}") {{ issues(first: 100{cursor_part}) {{ nodes {{ id identifier title state {{ name }} }} pageInfo {{ hasNextPage endCursor }} }} }} }}'
        })
        all_issues.extend(resp["data"]["project"]["issues"]["nodes"])
        page_info = resp["data"]["project"]["issues"]["pageInfo"]
        if not page_info["hasNextPage"]:
            break
        after = page_info["endCursor"]

    # Filter to M4/M5 Done issues
    done_issues = [
        issue for issue in all_issues
        if re.match(r"^RD-M[45]-|^QA-M[45]-|^GATE-M[45]|^M[45] Epic", issue["title"])
        and issue["state"]["name"] == "Done"
    ]
    done_issues.sort(key=lambda x: x["identifier"])
    print(f"=== Found {len(done_issues)} Done tasks ===")

    # Step 2: Process each issue
    updated = 0
    failed = 0

    for issue in done_issues:
        iid = issue["id"]
        identifier = issue["identifier"]
        title = issue["title"]
        print(f"\nProcessing {identifier}: {title}")

        # List existing comments
        list_resp = linear_post({
            "query": "query($id: String!) { issue(id: $id) { comments { nodes { id body createdAt } } } }",
            "variables": {"id": iid},
        })
        existing = list_resp["data"]["issue"]["comments"]["nodes"]

        # Delete garbled comments
        deleted = 0
        for c in existing:
            if is_garbled(c["body"]):
                del_resp = linear_post({
                    "query": "mutation($id: String!) { commentDelete(id: $id) { success } }",
                    "variables": {"id": c["id"]},
                })
                if del_resp["data"]["commentDelete"]["success"]:
                    print(f"  Deleted garbled comment ({c['id']})")
                    deleted += 1
                time.sleep(0.2)

        # Send correct comment
        comment = get_comment(title, comments)
        add_resp = linear_post({
            "query": "mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }",
            "variables": {"issueId": iid, "body": comment},
        })
        if add_resp["data"]["commentCreate"]["success"]:
            print(f"  Sent correct comment ({deleted} garbled deleted)")
            updated += 1
        else:
            print("  FAIL: comment create failed")
            failed += 1
        time.sleep(0.2)

    print(f"\n=== Fix complete: {updated} success, {failed} failed out of {len(done_issues)} tasks ===")

if __name__ == "__main__":
    main()
