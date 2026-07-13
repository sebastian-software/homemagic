#!/usr/bin/env node

import "@matter/nodejs";
import { Environment, Logger } from "@matter/main";
import { GeneralCommissioning } from "@matter/main/clusters";
import { ManualPairingCodeCodec, QrPairingCodeCodec } from "@matter/types";
import { CommissioningController } from "@project-chip/matter.js";
import { installRustStorage } from "./storage.mjs";

const MATTER_JS_REVISION = "b539372ff41fea24344760d69172508e9df931a2";
const NODE_VERSION = "v24.18.0";
const MAX_FRAME_BYTES = 1024 * 1024;
const MAX_SECRET_FRAME_BYTES = 8 * MAX_FRAME_BYTES;
const FABRIC_MARKER_HANDLE = "matter/storage/__fabric__";
const methods = [
    "fabric_load",
    "fabric_create",
    "node_commission",
    "node_inventory",
    "node_remove",
    "health_check",
    "process_drain",
];

if (process.version !== NODE_VERSION) process.exit(78);
Logger.level = "fatal";

let accepted;
let controller;
let buffer = Buffer.alloc(0);
let processing = Promise.resolve();
let reverseSequence = 0;
const pendingReverse = new Map();

const writeFrame = (value, limit = MAX_FRAME_BYTES) => {
    const payload = Buffer.from(JSON.stringify(value));
    if (payload.length === 0 || payload.length > limit) process.exit(74);
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

const partial = (request, phase, body) => {
    writeFrame({
        type: "response",
        payload: {
            session: request.session,
            request_id: request.request_id,
            disposition: { status: "partial", phase, body },
        },
    });
};

const reverseCall = (method, handle, value, expectedRevision) =>
    new Promise((resolve, reject) => {
        const requestId = `reverse-${++reverseSequence}`;
        pendingReverse.set(requestId, { resolve, reject });
        writeFrame({
            type: "secret_request",
            payload: {
                session: {
                    child_nonce: accepted.child_nonce,
                    session_nonce: accepted.session_nonce,
                },
                request_id: requestId,
                method,
                handle,
                expected_revision: expectedRevision,
                value: value === undefined ? undefined : Array.from(value),
            },
        }, MAX_SECRET_FRAME_BYTES);
    });

const requireSecretResult = disposition => {
    if (disposition?.status === "unavailable") throw new Error("secret_backend_unavailable");
    if (disposition?.status === "conflict") throw new Error("secret_revision_conflict");
    return disposition;
};

const secretBridge = {
    async get(handle) {
        const result = requireSecretResult(await reverseCall("get", handle));
        if (result.status === "missing") return undefined;
        if (result.status !== "found") throw new Error("secret_response_invalid");
        return { revision: result.record.revision, value: Uint8Array.from(result.record.value) };
    },
    async put(handle, value) {
        const result = requireSecretResult(await reverseCall("put", handle, value));
        if (result.status !== "stored") throw new Error("secret_response_invalid");
        return result.revision;
    },
    async compareAndSwap(handle, revision, value) {
        const result = requireSecretResult(await reverseCall("compare_and_swap", handle, value, revision));
        if (result.status !== "stored") throw new Error("secret_response_invalid");
        return result.revision;
    },
};

const ensureController = async () => {
    if (controller !== undefined) return controller;
    controller = new CommissioningController({
        environment: { environment: Environment.default, id: "homemagic-matter-sidecar" },
        adminFabricLabel: "HomeMagic",
        autoConnect: false,
        autoSubscribe: false,
    });
    await controller.start();
    return controller;
};

const decodeSetupPayload = payload => {
    if (!Array.isArray(payload) || payload.length === 0 || payload.length > 1024) return undefined;
    if (!payload.every(value => Number.isInteger(value) && value >= 0 && value <= 255)) return undefined;
    const setupCode = Buffer.from(payload).toString("utf8");
    try {
        if (setupCode.startsWith("MT:")) {
            const decoded = QrPairingCodeCodec.decode(setupCode);
            if (decoded.length !== 1) return undefined;
            return {
                passcode: decoded[0].passcode,
                identifierData: { longDiscriminator: decoded[0].discriminator },
            };
        }
        const decoded = ManualPairingCodeCodec.decode(setupCode);
        return {
            passcode: decoded.passcode,
            identifierData: { shortDiscriminator: decoded.shortDiscriminator },
        };
    } catch {
        return undefined;
    }
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
        installRustStorage(Environment.default, secretBridge);
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

    let body;
    if (request.method === "health_check") {
        body = {
            sdk_loaded: typeof CommissioningController === "function",
            environment_ready: Environment.default !== undefined,
            controller_started: controller !== undefined,
            matter_js_revision: MATTER_JS_REVISION,
            node_version: process.version,
        };
    } else if (request.method === "fabric_load") {
        const marker = await secretBridge.get(FABRIC_MARKER_HANDLE);
        if (marker === undefined) {
            fail(request, "fabric_not_found");
            return;
        }
        const active = await ensureController();
        body = { fabric_ready: true, commissioned_nodes: active.getCommissionedNodes().length };
    } else if (request.method === "fabric_create") {
        const active = await ensureController();
        if ((await secretBridge.get(FABRIC_MARKER_HANDLE)) === undefined) {
            await secretBridge.put(FABRIC_MARKER_HANDLE, Buffer.from("v1"));
        }
        body = { fabric_ready: true, commissioned_nodes: active.getCommissionedNodes().length };
    } else if (request.method === "node_commission") {
        const setup = decodeSetupPayload(request.body?.setup_payload);
        if (setup === undefined) {
            fail(request, "invalid_setup_payload");
            return;
        }
        const active = await ensureController();
        const discovery = {
            identifierData: setup.identifierData,
            discoveryCapabilities: { ble: false },
        };
        const knownAddress = request.body?.known_address;
        if (knownAddress !== undefined) {
            if (
                typeof knownAddress?.ip !== "string" ||
                knownAddress.ip.length === 0 ||
                knownAddress.ip.length > 255 ||
                !Number.isInteger(knownAddress?.port) ||
                knownAddress.port <= 0 ||
                knownAddress.port > 65535
            ) {
                fail(request, "invalid_known_address");
                return;
            }
            discovery.knownAddress = { ip: knownAddress.ip, port: knownAddress.port, type: "udp" };
        }
        try {
            const nodeId = await active.commissionNode(
                {
                    commissioning: {
                        regulatoryLocation: GeneralCommissioning.RegulatoryLocationType.IndoorOutdoor,
                        regulatoryCountryCode: "XX",
                    },
                    discovery,
                    passcode: setup.passcode,
                    autoSubscribe: false,
                },
                { connectNodeAfterCommissioning: false },
            );
            body = { node_id: nodeId.toString(), commissioned: true };
        } catch {
            partial(request, "commissioning_dispatched", {
                reconciliation: "inventory_required",
            });
            return;
        }
    } else if (request.method === "node_inventory") {
        const active = await ensureController();
        body = {
            nodes: active.getCommissionedNodesDetails().map(details => ({
                node_id: details.nodeId.toString(),
                advertised_name: details.advertisedName,
                operational_address: details.operationalAddress,
            })),
        };
    } else if (request.method === "node_remove") {
        const nodeId = request.body?.node_id;
        if (typeof nodeId !== "string" || !/^[1-9][0-9]*$/.test(nodeId)) {
            fail(request, "invalid_node_id");
            return;
        }
        const active = await ensureController();
        const matterNodeId = BigInt(nodeId);
        if (!active.getCommissionedNodes().some(candidate => candidate === matterNodeId)) {
            fail(request, "node_not_found");
            return;
        }
        try {
            const node = await active.getNode(matterNodeId);
            node.connect();
            if (!node.initialized) await node.events.initialized;
            await active.removeNode(matterNodeId, true);
        } catch {
            partial(request, "node_remove_dispatched", {
                node_id: nodeId,
                reconciliation: "inventory_required",
            });
            return;
        }
        body = { node_id: nodeId, removed: true };
    } else {
        body = {};
    }
    if (request.method === "process_drain") {
        await controller?.close();
    }
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
        if (length === 0 || length > MAX_SECRET_FRAME_BYTES) process.exit(74);
        if (buffer.length < length + 4) break;
        const payload = buffer.subarray(4, length + 4);
        buffer = buffer.subarray(length + 4);
        let frame;
        try {
            frame = JSON.parse(payload.toString("utf8"));
        } catch {
            process.exit(76);
        }
        if (frame?.type !== "secret_response" && length > MAX_FRAME_BYTES) process.exit(74);
        if (frame?.type === "secret_response") {
            const response = frame.payload;
            const pending = pendingReverse.get(response?.request_id);
            if (
                pending === undefined ||
                response?.session?.child_nonce !== accepted?.child_nonce ||
                response?.session?.session_nonce !== accepted?.session_nonce
            ) {
                process.exit(76);
            }
            pendingReverse.delete(response.request_id);
            pending.resolve(response.disposition);
        } else {
            processing = processing.then(() => handleFrame(frame)).catch(() => process.exit(70));
        }
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
        max_secret_frame_bytes: MAX_SECRET_FRAME_BYTES,
        event_window: 64,
    },
    child_nonce: "matter-js-child-v1",
});
