// ============================================================
// src/lib/bridge-provider.ts
// Selects the bridge implementation based on the environment.
//  - Real device / daemon-served UI → wsBridge (typed WebSocket to the daemon)
//  - Browser dev → mockBridge
// Override with VITE_BRIDGE_MODE = "ksu" | "mock".
// ============================================================
import type { Bridge } from "./bridge";
import { getRuntimeBridgeMode } from "./ksu-webui";

type BridgeMode = "ksu" | "mock";

let loadedBridge: Bridge | null = null;
let loadingBridge: Promise<Bridge> | null = null;

function pickMode(): BridgeMode {
  const mode = import.meta.env.VITE_BRIDGE_MODE;
  if (mode === "mock" || mode === "ksu") return mode;
  return getRuntimeBridgeMode() === "mock" ? "mock" : "ksu";
}

async function loadBridge(): Promise<Bridge> {
  if (loadedBridge) return loadedBridge;
  if (!loadingBridge) {
    const mode = pickMode();
    loadingBridge =
      mode === "ksu"
        ? import("./ws-bridge").then((module) => module.wsBridge)
        : import("./mock-bridge").then((module) => module.mockBridge);
    loadingBridge = loadingBridge.then((bridge) => {
      loadedBridge = bridge;
      return bridge;
    });
  }
  return loadingBridge;
}

export const bridge: Bridge = {
  async start(profileId) {
    return (await loadBridge()).start(profileId);
  },
  async stop() {
    return (await loadBridge()).stop();
  },
  async restart() {
    return (await loadBridge()).restart();
  },
  async status() {
    return (await loadBridge()).status();
  },
  onStatus(cb) {
    let unsubscribed = false;
    let dispose: (() => void) | null = null;

    void loadBridge().then((impl) => {
      if (unsubscribed) return;
      dispose = impl.onStatus(cb);
    });

    return () => {
      unsubscribed = true;
      dispose?.();
    };
  },
  async capabilities() {
    return (await loadBridge()).capabilities();
  },
  async ping(profileId) {
    return (await loadBridge()).ping(profileId);
  },
  async pingAll(ids, onResult) {
    return (await loadBridge()).pingAll(ids, onResult);
  },
  async realPing(profileId) {
    return (await loadBridge()).realPing(profileId);
  },
  async realPingAll(ids, onResult) {
    return (await loadBridge()).realPingAll(ids, onResult);
  },
  async speedTest(profileId) {
    return (await loadBridge()).speedTest(profileId);
  },
  async speedTestAll(ids, onResult) {
    return (await loadBridge()).speedTestAll(ids, onResult);
  },
  async log(input) {
    return (await loadBridge()).log(input);
  },
  async testLog(profileId, kind) {
    return (await loadBridge()).testLog(profileId, kind);
  },
  async clearLogs() {
    return (await loadBridge()).clearLogs();
  },
  async readState() {
    return (await loadBridge()).readState();
  },
  async mutate(intent) {
    return (await loadBridge()).mutate(intent);
  },
  onAssetsUpdated(cb) {
    let unsubscribed = false;
    let dispose: (() => void) | null = null;

    void loadBridge().then((impl) => {
      if (unsubscribed) return;
      dispose = impl.onAssetsUpdated(cb);
    });

    return () => {
      unsubscribed = true;
      dispose?.();
    };
  },
  async downloadAsset(filename, url, mode) {
    return (await loadBridge()).downloadAsset(filename, url, mode);
  },
  async listAssets() {
    return (await loadBridge()).listAssets();
  },
  async listApps() {
    return (await loadBridge()).listApps();
  },
  async reloadAppFilter() {
    return (await loadBridge()).reloadAppFilter();
  },
  async chainCandidates(profile) {
    return (await loadBridge()).chainCandidates(profile);
  },
  async parseShareLinks(text) {
    return (await loadBridge()).parseShareLinks(text);
  },
  async buildShareLink(profile) {
    return (await loadBridge()).buildShareLink(profile);
  },
  async exportBackup() {
    return (await loadBridge()).exportBackup();
  },
  async importBackup(file, mode) {
    return (await loadBridge()).importBackup(file, mode);
  },
};
