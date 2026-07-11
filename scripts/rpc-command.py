#!/usr/bin/env python3
"""Resolve one device by query and validate or execute an on/off command."""

import argparse
import datetime
import json
import os
import sys
import urllib.error
import urllib.request
import uuid


def rpc(url: str, token: str, method: str, params: dict) -> dict:
    body = json.dumps(
        {"jsonrpc": "2.0", "id": str(uuid.uuid4()), "method": method, "params": params}
    ).encode()
    request = urllib.request.Request(
        f"{url.rstrip('/')}/rpc",
        data=body,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        payload = json.load(response)
    if "error" in payload:
        raise RuntimeError(json.dumps(payload["error"], sort_keys=True))
    return payload["result"]


def searchable(device: dict) -> str:
    snapshot = device["snapshot"]
    values = [snapshot.get("name", ""), snapshot.get("native_id", "")]
    values.extend(device.get("aliases", []))
    return " ".join(values).casefold()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device", help="Unique substring of device name, alias, or native ID")
    parser.add_argument("--url", default="http://127.0.0.1:8787")
    parser.add_argument("--endpoint", help="Endpoint ID; defaults to the first switch/light endpoint")
    parser.add_argument("--action", choices=("on", "off", "toggle"), default="toggle")
    parser.add_argument("--deadline-seconds", type=int, default=15)
    parser.add_argument("--idempotency-key", default=None)
    parser.add_argument(
        "--execute",
        action="store_true",
        help="Physically dispatch; the default only calls commands.validate",
    )
    args = parser.parse_args()
    token = os.environ.get("HOMEMAGIC_TOKEN")
    if not token:
        parser.error("HOMEMAGIC_TOKEN must contain an actor bearer token")

    devices = rpc(args.url, token, "devices.list", {"integration": "shelly"})["devices"]
    matches = [item["device"] for item in devices if args.device.casefold() in searchable(item["device"])]
    if len(matches) != 1:
        names = [item["snapshot"].get("name", item["snapshot"]["id"]) for item in matches]
        raise RuntimeError(f"device query must match exactly one device; matches={names}")
    device = matches[0]
    endpoints = device["snapshot"].get("endpoints", [])
    if args.endpoint:
        endpoints = [item for item in endpoints if item["id"] == args.endpoint]
    else:
        endpoints = [item for item in endpoints if item["id"].startswith(("switch:", "light:"))]
    if len(endpoints) != 1:
        raise RuntimeError("endpoint selection must resolve exactly one switch or light endpoint")

    action = "toggle" if args.action == "toggle" else "set"
    command = {"action": action}
    if action == "set":
        command["on"] = args.action == "on"
    deadline = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(
        seconds=args.deadline_seconds
    )
    params = {
        "device_id": device["snapshot"]["id"],
        "endpoint_id": endpoints[0]["id"],
        "payload": {"capability": "on_off", "command": command},
        "idempotency_key": args.idempotency_key or str(uuid.uuid4()),
        "deadline": deadline.isoformat().replace("+00:00", "Z"),
    }
    method = "commands.execute" if args.execute else "commands.validate"
    print(json.dumps(rpc(args.url, token, method, params), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, urllib.error.URLError) as error:
        print(f"rpc-command: {error}", file=sys.stderr)
        raise SystemExit(1) from error
