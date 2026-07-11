#!/usr/bin/env python3
"""Run one redacted, cleanup-first Shelly command hardware scenario."""

import argparse
import datetime
import json
import os
import pathlib
import platform
import sys
import time
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
    with urllib.request.urlopen(request, timeout=20) as response:
        payload = json.load(response)
    if "error" in payload:
        raise RuntimeError(json.dumps(payload["error"], sort_keys=True))
    return payload["result"]


def resolve(url: str, token: str, query: str, capability: str) -> tuple[dict, dict]:
    rpc(url, token, "devices.refresh", {})
    items = rpc(url, token, "devices.list", {"integration": "shelly"})["devices"]
    needle = query.casefold()
    devices = []
    for item in items:
        device = item["device"]
        snapshot = device["snapshot"]
        searchable = " ".join(
            [snapshot.get("name", ""), snapshot.get("native_id", ""), *device.get("aliases", [])]
        ).casefold()
        if needle in searchable:
            devices.append(device)
    if len(devices) != 1:
        raise RuntimeError(f"device query must match exactly one Shelly device; matched={len(devices)}")
    endpoints = [
        endpoint
        for endpoint in devices[0]["snapshot"].get("endpoints", [])
        if any(value.get("kind") == capability for value in endpoint.get("capabilities", []))
    ]
    if len(endpoints) != 1:
        raise RuntimeError(f"device must expose exactly one {capability} endpoint; matched={len(endpoints)}")
    return devices[0], endpoints[0]


def observed(url: str, token: str, device_id: str, endpoint_id: str, capability: str) -> dict:
    details = rpc(url, token, "devices.get", {"id": device_id})["device"]
    matches = [
        item
        for item in details["observations"]
        if item["endpoint_id"] == endpoint_id and item["capability"]["name"] == capability
    ]
    if len(matches) != 1:
        raise RuntimeError(f"missing unique current {capability} observation")
    return {name: value["value"] for name, value in matches[0]["values"].items()}


def command(
    url: str,
    token: str,
    execute: bool,
    device_id: str,
    endpoint_id: str,
    payload: dict,
    label: str,
) -> dict:
    deadline = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=15)
    result = rpc(
        url,
        token,
        "commands.execute" if execute else "commands.validate",
        {
            "device_id": device_id,
            "endpoint_id": endpoint_id,
            "payload": payload,
            "idempotency_key": f"hardware-smoke-{label}-{uuid.uuid4()}",
            "deadline": deadline.isoformat().replace("+00:00", "Z"),
        },
    )["command"]
    expected = "confirmed" if execute else "validated"
    if result["state"] != expected:
        raise RuntimeError(f"{label} ended in {result['state']}: {result.get('failure')}")
    return {
        "label": label,
        "command_id": result["envelope"]["id"],
        "state": result["state"],
        "policy_allowed": result.get("policy", {}).get("allowed"),
        "acknowledged": result.get("acknowledgement") is not None,
        "observed_confirmation": result.get("confirmation") is not None,
    }


def firmware(device: dict) -> str | None:
    for endpoint in device["snapshot"].get("endpoints", []):
        for capability in endpoint.get("capabilities", []):
            if capability.get("kind") == "diagnostics":
                return capability.get("firmware_version")
    return None


def write_report(path: pathlib.Path, report: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scenario", choices=("switch", "dimmer", "cover"))
    parser.add_argument("device", help="Unique substring of device name, alias, or native ID")
    parser.add_argument("--url", default="http://127.0.0.1:8787")
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--execute", action="store_true", help="Perform physical commands")
    parser.add_argument(
        "--physical-stop-confirmed",
        action="store_true",
        help="Required with --execute for cover motion after checking the physical stop path",
    )
    args = parser.parse_args()
    token = os.environ.get("HOMEMAGIC_TOKEN")
    if not token:
        parser.error("HOMEMAGIC_TOKEN must contain the dedicated hardware-test actor token")
    if args.scenario == "cover" and args.execute and not args.physical_stop_confirmed:
        parser.error("cover execution requires --physical-stop-confirmed")

    capability = {"switch": "on_off", "dimmer": "level", "cover": "position"}[args.scenario]
    report = {
        "schema": "homemagic.command_hardware_smoke.v1",
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "host": {"operating_system": platform.system().lower(), "architecture": platform.machine()},
        "integration": "shelly",
        "scenario": args.scenario,
        "mode": "execute" if args.execute else "validate",
        "commands": [],
        "cleanup": {"attempted": False, "verified": False},
        "result": "failed",
        "redaction": "device/native IDs, names, addresses, aliases, spaces, credentials, and vendor payloads omitted",
    }
    device_id = endpoint_id = None
    original = None
    try:
        device, endpoint = resolve(args.url, token, args.device, capability)
        device_id = device["snapshot"]["id"]
        endpoint_id = endpoint["id"]
        report["device"] = {
            "manufacturer": device["snapshot"].get("manufacturer"),
            "model": device["snapshot"].get("model"),
            "firmware": firmware(device),
            "capability": f"{capability}.v1",
        }
        original = observed(args.url, token, device_id, endpoint_id, capability)
        if args.scenario == "switch":
            target = not bool(original["on"])
            report["commands"].append(command(args.url, token, args.execute, device_id, endpoint_id,
                {"capability": "on_off", "command": {"action": "set", "on": target}}, "switch-change"))
        elif args.scenario == "dimmer":
            current = round(float(original["percent"]))
            target = min(100, current + 10) if current <= 50 else max(1, current - 10)
            report["commands"].append(command(args.url, token, args.execute, device_id, endpoint_id,
                {"capability": "level", "command": {"percent": target, "transition_ms": 250}}, "dimmer-change"))
        else:
            if original.get("percent") is None:
                raise RuntimeError("cover scenario requires calibrated current position")
            report["commands"].append(command(args.url, token, args.execute, device_id, endpoint_id,
                {"capability": "position", "command": {"action": "stop"}}, "cover-emergency-stop-first"))
            for action in ("open", "close"):
                report["commands"].append(command(args.url, token, args.execute, device_id, endpoint_id,
                    {"capability": "position", "command": {"action": action}}, f"cover-{action}"))
                if args.execute:
                    time.sleep(0.5)
                report["commands"].append(command(args.url, token, args.execute, device_id, endpoint_id,
                    {"capability": "position", "command": {"action": "stop"}}, f"cover-{action}-stop"))
            current = round(float(original["percent"]))
            target = min(100, current + 3) if current <= 50 else max(0, current - 3)
            report["commands"].append(command(args.url, token, args.execute, device_id, endpoint_id,
                {"capability": "position", "command": {"action": "go_to", "percent": target}}, "cover-position"))
        report["result"] = "scenario_completed"
    except Exception as error:  # the report must survive every scenario failure
        report["error"] = str(error)
    finally:
        if args.execute and device_id and endpoint_id and original is not None:
            report["cleanup"]["attempted"] = True
            try:
                if args.scenario == "switch":
                    payload = {"capability": "on_off", "command": {"action": "set", "on": bool(original["on"])}}
                elif args.scenario == "dimmer":
                    payload = {"capability": "level", "command": {"percent": round(float(original["percent"])), "transition_ms": 250}}
                else:
                    command(args.url, token, True, device_id, endpoint_id,
                        {"capability": "position", "command": {"action": "stop"}}, "cleanup-stop")
                    payload = {"capability": "position", "command": {"action": "go_to", "percent": round(float(original["percent"]))}}
                report["cleanup"]["command"] = command(
                    args.url, token, True, device_id, endpoint_id, payload, "restore-original"
                )
                restored = observed(args.url, token, device_id, endpoint_id, capability)
                field = {"switch": "on", "dimmer": "percent", "cover": "percent"}[args.scenario]
                tolerance = 0 if field == "on" else 1
                report["cleanup"]["verified"] = (
                    restored[field] == original[field]
                    if tolerance == 0
                    else abs(float(restored[field]) - float(original[field])) <= tolerance
                )
            except Exception as cleanup_error:
                report["cleanup"]["error"] = str(cleanup_error)
        elif not args.execute:
            report["cleanup"] = {"attempted": False, "verified": True, "reason": "validation_only"}
        if report["result"] == "scenario_completed" and not report["cleanup"]["verified"]:
            report["result"] = "cleanup_failed"
        write_report(args.output, report)

    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["result"] == "scenario_completed" and report["cleanup"]["verified"] else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, urllib.error.URLError) as error:
        print(f"hardware-command-smoke: {error}", file=sys.stderr)
        raise SystemExit(1) from error
