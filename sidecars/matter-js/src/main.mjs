#!/usr/bin/env node

import "@matter/nodejs";
import { Environment } from "@matter/main";
import { CommissioningController } from "@project-chip/matter.js";

const MATTER_JS_REVISION = "b539372ff41fea24344760d69172508e9df931a2";
const NODE_VERSION = "v24.18.0";
const MAX_FRAME_BYTES = 1024 * 1024;
const methods = ["health_check", "process_drain"];

if (process.version !== NODE_VERSION) process.exit(78);

let accepted;
let buffer = Buffer.alloc(0);
let processing = Promise.resolve();

const writeFrame = value => {
    const payload = Buffer.from(JSON.stringify(value));
    if (payload.length === 0 || payload.length > MAX_FRAME_BYTES) process.exit(74);
    const header = Buffer.allocUnsafe(4);
    header.writeUInt32BE(payload.length);
    process.stdout.write(header);
    process.stdout.write(payload);
};

const fail = (request, code) => {
    writeFrame({
        type: "response",
        payload: {
            session: request.session,
            request_id: request.request_id,
            disposition: { status: "error", error: { code, retryable: false } },
        },
    });
};

const handleFrame = async frame => {
    if (accepted === undefined) {
        if (frame?.type !== "accept") process.exit(76);
        const value = frame.payload;
        if (
            value?.protocol?.major !== 1 ||
            value?.protocol?.minor !== 0 ||
            value?.child_nonce !== "matter-js-child-v1" ||
            typeof value?.session_nonce !== "string"
        ) {
            process.exit(76);
        }
        accepted = value;
        return;
    }

    if (frame?.type !== "request") process.exit(76);
    const request = frame.payload;
    if (
        request?.session?.child_nonce !== accepted.child_nonce ||
        request?.session?.session_nonce !== accepted.session_nonce
    ) {
        process.exit(76);
    }
    if (!methods.includes(request.method)) {
        fail(request, "unsupported_method");
        return;
    }

    const body =
        request.method === "health_check"
            ? {
                  sdk_loaded: typeof CommissioningController === "function",
                  environment_ready: Environment.default !== undefined,
                  matter_js_revision: MATTER_JS_REVISION,
                  node_version: process.version,
              }
            : {};
    writeFrame({
        type: "response",
        payload: {
            session: request.session,
            request_id: request.request_id,
            disposition: { status: "result", body },
        },
    });
    if (request.method === "process_drain") {
        process.stdin.pause();
        process.stdout.end(() => process.exit(0));
    }
};

process.stdin.on("data", chunk => {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 4) {
        const length = buffer.readUInt32BE(0);
        if (length === 0 || length > MAX_FRAME_BYTES) process.exit(74);
        if (buffer.length < length + 4) break;
        const payload = buffer.subarray(4, length + 4);
        buffer = buffer.subarray(length + 4);
        let frame;
        try {
            frame = JSON.parse(payload.toString("utf8"));
        } catch {
            process.exit(76);
        }
        processing = processing.then(() => handleFrame(frame)).catch(() => process.exit(70));
    }
});
process.stdin.on("end", () => process.exit(0));
process.stdin.on("error", () => process.exit(74));
process.stdout.on("error", () => process.exit(74));

writeFrame({
    protocol: { major: 1, minor: 0 },
    matter_js_revision: MATTER_JS_REVISION,
    node_version: process.version,
    methods,
    event_kinds: [],
    limits: {
        max_frame_bytes: MAX_FRAME_BYTES,
        max_secret_frame_bytes: 8 * MAX_FRAME_BYTES,
        event_window: 64,
    },
    child_nonce: "matter-js-child-v1",
});
