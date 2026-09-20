#!/usr/bin/env python3
"""Searches a project through the RustRover MCP server and prints dense results.

usage: ide_search.py text|regex|symbol|file <query> [--project <path>] [--limit N]
                     [--in <glob>]... [--external] [--context N]

The server is the `rustrover` entry of user-scope `~/.claude.json`. Text and
regex hits are grouped per file as `line:col  matched line`; symbol hits show
the declaration's line span and first line; file hits list paths. `--in`
takes project-relative globs, repeatable (`--in 'crates/**' --in '!**/tests/**'`).
`--context N` adds N lines around each text or regex hit.
"""
import argparse
import json
import os
import pathlib
import sys
import urllib.request

KIND_TO_TOOL = {
    "text": "search_text",
    "regex": "search_regex",
    "symbol": "search_symbol",
    "file": "search_file",
}


def server_url() -> str:
    config = json.load(open(os.path.expanduser("~/.claude.json")))
    server = (config.get("mcpServers") or {}).get("rustrover")
    if not server or "url" not in server:
        raise SystemExit("no user-scope `rustrover` MCP server in ~/.claude.json")
    return server["url"]


class Session:
    def __init__(self, url: str):
        self.url = url
        self.session_id = None
        self.next_id = 1
        self.call("initialize", {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "ide_search", "version": "1"},
        })
        self.notify("notifications/initialized")

    def _post(self, body: dict) -> tuple[dict | None, dict]:
        data = json.dumps(body).encode()
        request = urllib.request.Request(self.url, data=data, method="POST")
        request.add_header("Content-Type", "application/json")
        request.add_header("Accept", "application/json, text/event-stream")
        if self.session_id:
            request.add_header("Mcp-Session-Id", self.session_id)
        with urllib.request.urlopen(request, timeout=120) as response:
            headers = dict(response.headers)
            session = response.headers.get("Mcp-Session-Id")
            if session:
                self.session_id = session
            raw = response.read().decode()
            content_type = response.headers.get("Content-Type", "")
        if not raw.strip():
            return None, headers
        if "text/event-stream" in content_type:
            for line in raw.splitlines():
                if line.startswith("data:"):
                    payload = line[len("data:"):].strip()
                    if payload:
                        message = json.loads(payload)
                        if "result" in message or "error" in message:
                            return message, headers
            return None, headers
        return json.loads(raw), headers

    def call(self, method: str, params: dict) -> dict:
        body = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}
        self.next_id += 1
        message, _ = self._post(body)
        if message is None:
            raise SystemExit(f"{method}: empty response")
        if "error" in message:
            raise SystemExit(f"{method}: {message['error']}")
        return message["result"]

    def notify(self, method: str) -> None:
        self._post({"jsonrpc": "2.0", "method": method})

    def tool(self, name: str, arguments: dict) -> dict:
        result = self.call("tools/call", {"name": name, "arguments": arguments})
        texts = [part.get("text", "") for part in result.get("content", []) if part.get("type") == "text"]
        joined = "\n".join(texts).strip()
        if result.get("isError"):
            raise SystemExit(f"{name}: {joined}")
        try:
            return json.loads(joined)
        except json.JSONDecodeError:
            return {"raw": joined}


def read_lines(root: pathlib.Path, rel: str) -> list[str]:
    try:
        return (root / rel).read_text(errors="replace").split("\n")
    except OSError:
        return []


def print_matches(root: pathlib.Path, items: list[dict], context: int) -> None:
    by_file: dict[str, list[dict]] = {}
    for item in items:
        by_file.setdefault(item["filePath"], []).append(item)
    for rel, hits in by_file.items():
        lines = read_lines(root, rel)
        print(f"{rel}  ({len(hits)})")
        shown = set()
        for hit in hits:
            line = hit["startLine"]
            column = hit.get("startColumn", 1)
            first = max(1, line - context)
            last = min(len(lines), line + context)
            for number in range(first, last + 1):
                if number in shown and context:
                    continue
                shown.add(number)
                text = lines[number - 1].rstrip() if number - 1 < len(lines) else ""
                marker = f"{number}:{column}" if number == line else f"{number}"
                print(f"  {marker:>8}  {text}")
            if context:
                print("  --")
    print(f"{len(items)} match(es) in {len(by_file)} file(s)")


def print_symbols(root: pathlib.Path, items: list[dict]) -> None:
    for item in items:
        rel = item["filePath"]
        lines = read_lines(root, rel)
        start, end = item["startLine"], item.get("endLine", item["startLine"])
        text = lines[start - 1].strip() if start - 1 < len(lines) else ""
        span = f"{start}" if end == start else f"{start}-{end}"
        print(f"{rel}:{span}  {text}")
    print(f"{len(items)} symbol(s)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("kind", choices=KIND_TO_TOOL)
    parser.add_argument("query")
    parser.add_argument("--project", default=os.getcwd())
    parser.add_argument("--limit", type=int, default=100)
    parser.add_argument("--in", dest="paths", action="append", default=[])
    parser.add_argument("--external", action="store_true")
    parser.add_argument("--context", type=int, default=0)
    args = parser.parse_args()
    root = pathlib.Path(args.project).resolve()

    arguments = {"q": args.query, "projectPath": str(root), "limit": args.limit}
    if args.paths:
        arguments["paths"] = args.paths
    if args.kind == "symbol" and args.external:
        arguments["include_external"] = True

    session = Session(server_url())
    result = session.tool(KIND_TO_TOOL[args.kind], arguments)
    items = result.get("items", result.get("files", []))
    if "raw" in result:
        print(result["raw"])
        return 0
    if args.kind == "symbol":
        print_symbols(root, items)
    elif args.kind == "file":
        for item in items:
            print(item if isinstance(item, str) else item.get("filePath", json.dumps(item)))
        print(f"{len(items)} file(s)")
    else:
        print_matches(root, items, args.context)
    return 0


if __name__ == "__main__":
    sys.exit(main())
