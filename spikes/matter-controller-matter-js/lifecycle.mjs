#!/usr/bin/env node

import "@matter/nodejs";

import { Environment, Logger } from "@matter/main";
import { OnOffClient } from "@matter/main/behaviors/on-off";
import { BasicInformationCluster, GeneralCommissioning } from "@matter/main/clusters";
import { ControllerCommissioningFlow, PeerSet } from "@matter/protocol";
import { CommissioningController } from "@project-chip/matter.js";
import { writeFile } from "node:fs/promises";

Logger.level = "fatal";

const [mode, reportPath, address = "::1", portText = "55540"] = process.argv.slice(2);
const port = Number(portText);
const operationalAddressFallback = process.env.HOMEMAGIC_MATTER_OPERATIONAL_ADDRESS_FALLBACK === "1";
const outcomes = {
    fabric_create: "not_run",
    commission: "not_run",
    inventory: "not_run",
    read: "not_run",
    subscribe: "not_run",
    invoke: "not_run",
    restart: "not_run",
    remove: "not_run",
};
const report = {
    schema: "homemagic.matter.matter-js-independent-reference.v1",
    mode,
    outcomes,
    active_phase: "initialize",
    error: null,
};

const persist = async () => writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
const fail = async (phase, error) => {
    outcomes[phase] = "fail";
    report.error = {
        phase,
        name: error?.name ?? "Error",
        message: String(error?.message ?? error),
    };
    await persist();
};

class InstrumentedCommissioningFlow extends ControllerCommissioningFlow {
    constructor(interaction, ca, fabric, commissioningOptions, transitionToCase) {
        const instrumentedTransition = async (peerAddress, supportsConcurrentConnections) => {
            if (operationalAddressFallback) {
                const peer = controller.env.get(PeerSet).for(peerAddress);
                peer.descriptor.operationalAddress = { ip: address, port, type: "udp" };
                report.operational_address_fallback = "applied";
                await persist();
            }
            return transitionToCase(peerAddress, supportsConcurrentConnections);
        };
        super(interaction, ca, fabric, commissioningOptions, instrumentedTransition);
        report.commissioning_stages = [];
        for (const step of this.commissioningSteps) {
            const execute = step.stepLogic;
            step.stepLogic = async () => {
                const stage = {
                    step: `${step.stepNumber}.${step.subStepNumber}`,
                    name: step.name,
                    status: "started",
                };
                report.commissioning_stages.push(stage);
                await persist();
                try {
                    const result = await execute();
                    stage.status = "completed";
                    await persist();
                    return result;
                } catch (error) {
                    stage.status = "failed";
                    stage.error_name = error?.name ?? "Error";
                    await persist();
                    throw error;
                }
            };
        }
    }
}

const watchdog = setTimeout(() => {
    outcomes.timeout = "fail";
    report.error = {
        phase: report.active_phase,
        name: "TimeoutError",
        message: "Lifecycle process exceeded the 180 second evidence budget",
    };
    persist().finally(() => process.exit(0));
}, 180_000);

let controller;
try {
    const environment = Environment.default;
    controller = new CommissioningController({
        environment: { environment, id: "homemagic-matter-js-spike" },
        adminFabricLabel: "HomeMagic interop spike",
        autoConnect: false,
        autoSubscribe: true,
    });
    report.active_phase = "fabric_create";
    await controller.start();
    outcomes.fabric_create = "pass";

    let nodes = controller.getCommissionedNodes();
    if (mode === "commission") {
        if (nodes.length === 0) {
            try {
                report.active_phase = "commission";
                await controller.commissionNode(
                    {
                        commissioning: {
                            regulatoryLocation: GeneralCommissioning.RegulatoryLocationType.IndoorOutdoor,
                            regulatoryCountryCode: "XX",
                        },
                        discovery: {
                            knownAddress: { ip: address, port, type: "udp" },
                            identifierData: { longDiscriminator: 3840 },
                            discoveryCapabilities: { ble: false },
                        },
                        passcode: 20202021,
                        autoSubscribe: true,
                        subscribeMinIntervalFloorSeconds: 0,
                        subscribeMaxIntervalCeilingSeconds: 5,
                    },
                    {
                        connectNodeAfterCommissioning: false,
                        commissioningFlowImpl: InstrumentedCommissioningFlow,
                    },
                );
            } catch (error) {
                await fail("commission", error);
                throw error;
            }
            outcomes.commission = "pass";
            nodes = controller.getCommissionedNodes();
        } else {
            outcomes.commission = "pass";
        }
    } else {
        report.active_phase = "restart";
        outcomes.restart = nodes.length > 0 ? "pass" : "fail";
        if (nodes.length === 0) throw new Error("No commissioned node survived process restart");
    }

    report.active_phase = "inventory";
    outcomes.inventory = nodes.length === 1 ? "pass" : "fail";
    if (nodes.length !== 1) throw new Error(`Expected one commissioned node, found ${nodes.length}`);

    const node = await controller.getNode(nodes[0]);
    report.active_phase = "connect_and_subscribe";
    node.connect();
    if (!node.initialized) await node.events.initialized;

    const info = node.getRootClusterClient(BasicInformationCluster);
    if (info === undefined) throw new Error("Basic Information cluster missing");
    report.active_phase = "read";
    report.product_name = await info.getProductNameAttribute();
    outcomes.read = "pass";

    const endpoint = node.parts.get(1);
    if (endpoint === undefined) throw new Error("On/Off endpoint 1 missing");
    const state = endpoint.stateOf(OnOffClient);
    const commands = endpoint.commandsOf(OnOffClient);
    if (state === undefined || commands === undefined) throw new Error("On/Off client missing");

    if (mode === "commission") {
        const before = state.onOff;
        report.active_phase = "invoke";
        await commands.toggle();
        outcomes.invoke = "pass";
        await new Promise(resolve => setTimeout(resolve, 1_500));
        report.on_off_before = before;
        report.on_off_after = state.onOff;
        report.active_phase = "subscribe";
        outcomes.subscribe = state.onOff !== before ? "pass" : "fail";
    } else {
        report.active_phase = "remove";
        await controller.removeNode(nodes[0]);
        outcomes.remove = controller.getCommissionedNodes().length === 0 ? "pass" : "fail";
    }

    report.active_phase = null;
    await persist();
} catch (error) {
    if (report.error === null) {
        const phase = Object.entries(outcomes).find(([, value]) => value === "fail")?.[0] ?? "unknown";
        await fail(phase, error);
    }
} finally {
    clearTimeout(watchdog);
    try {
        await controller?.close();
    } catch {
        // The report already contains the protocol outcome; shutdown is best effort in the spike.
    }
}
