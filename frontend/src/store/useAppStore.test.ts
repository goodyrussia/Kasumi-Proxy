import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AssetFile, Profile } from "../generated/bindings";
import type {
  AdvancedSettings,
  AppState,
  AssetsUpdatedEvent,
  Bridge,
  LogTarget,
  ServiceStatus,
} from "../lib/bridge";
import { emptyProfile } from "../lib/profile-utils";

type Vless = Extract<Profile, { protocol: "vless" }>;
type UseAppStoreModule = typeof import("./useAppStore");
type BridgeMock = {
  [K in keyof Bridge]: ReturnType<typeof vi.fn<Bridge[K]>>;
};

import { applyMutation } from "../lib/apply-mutation";
import { uid } from "../lib/utils";
import { EMPTY_SETTINGS } from "./defaults";

const DEFAULT_SETTINGS: AdvancedSettings = {
  ...EMPTY_SETTINGS,
  autoStart: false,
};

const DEFAULT_STATUS: ServiceStatus = {
  state: "stopped",
  activeId: null,
  uploadBytes: 0,
  downloadBytes: 0,
  uptimeSec: 0,
  core: "Xray",
  pendingRestart: false,
};

// Nested-model builder: Vless is `meta`/`endpoint`/`transport`/`tls`/ root
// fields. Built from `emptyProfile("vless")` so every serde default is present;
// overrides accept any of those groups plus root credentials. `meta` is
// `Partial<Meta>` so a test can stub just `id`.
type VlessOverrides = Omit<Partial<Vless>, "meta" | "endpoint" | "transport" | "tls"> & {
  meta?: Partial<Vless["meta"]>;
  endpoint?: Partial<Vless["endpoint"]>;
  transport?: Vless["transport"];
  tls?: Partial<NonNullable<Vless["tls"]>>;
};
function makeVless(overrides: VlessOverrides = {}): Vless {
  const base = emptyProfile("vless") as Vless;
  return {
    ...base,
    ...overrides,
    meta: {
      ...base.meta,
      ...(overrides.meta ?? {}),
      id: overrides.meta?.id ?? uid(),
      remarks: overrides.meta?.remarks ?? "Node",
      groupId: overrides.meta?.groupId ?? "g-main",
    },
    endpoint: { ...base.endpoint, ...overrides.endpoint },
    transport: overrides.transport ?? base.transport,
    tls: overrides.tls ? { ...base.tls, ...overrides.tls } : base.tls,
  };
}

function makeAsset(overrides: Partial<AssetFile> = {}): AssetFile {
  return {
    id: overrides.id ?? uid(),
    remarks: overrides.remarks ?? "geoip.dat",
    url: overrides.url ?? "https://example.com/geoip.dat",
    lastUpdated: overrides.lastUpdated ?? Date.now(),
    locked: overrides.locked ?? false,
  };
}

function makeState(overrides: Partial<AppState> = {}): AppState {
  return {
    profiles: overrides.profiles ?? [],
    groups: overrides.groups ?? [
      { id: "g-main", name: "Main" },
      { id: "g-alt", name: "Alt" },
    ],
    routingRules: overrides.routingRules ?? [],
    assetFiles: overrides.assetFiles ?? [],
    settings: overrides.settings ?? DEFAULT_SETTINGS,
    activeId: overrides.activeId ?? null,
    version: overrides.version,
  };
}

function createBridgeMock(): BridgeMock {
  return {
    start: vi.fn(async (profileId: string) => ({
      ...DEFAULT_STATUS,
      state: "connected",
      activeId: profileId,
    })),
    stop: vi.fn(async () => DEFAULT_STATUS),
    restart: vi.fn(async () => ({ ...DEFAULT_STATUS, state: "connected" })),
    status: vi.fn(async () => DEFAULT_STATUS),
    onStatus: vi.fn((_cb: (s: ServiceStatus) => void) => () => {}),
    ping: vi.fn(async (_profileId: string) => 0),
    pingAll: vi.fn(async () => ({})),
    log: vi.fn(async (_input?: { target?: LogTarget; lines?: number }) => ""),
    testLog: vi.fn(async () => ""),
    clearLogs: vi.fn(async () => ({ ok: true })),
    realPing: vi.fn(async () => 0),
    realPingAll: vi.fn(async () => ({})),
    speedTest: vi.fn(async () => 0),
    speedTestAll: vi.fn(async () => ({})),
    capabilities: vi.fn(async () => ({
      bridge: "mock",
      xrayVersion: "",
      tun: false,
    })),
    listApps: vi.fn(async () => []),
    reloadAppFilter: vi.fn(async () => ({ ok: true })),
    readState: vi.fn(async () => makeState()),
    // Wired in beforeEach to run the real applyMutation against the live store
    // state (so tests validate the canonical mutation logic too).
    mutate: vi.fn(async (_intent) => makeState()),
    onAssetsUpdated: vi.fn((_cb: (info: AssetsUpdatedEvent) => void) => () => {}),
    downloadAsset: vi.fn(
      async (_filename: string, _url: string, _mode?: "auto" | "proxy" | "direct") => ({
        ok: true,
      }),
    ),
    listAssets: vi.fn(async () => []),
    chainCandidates: vi.fn(async (_profile: Profile) => []),
    parseShareLinks: vi.fn(async (_text: string) => []),
    buildShareLink: vi.fn(async (_profile: Profile) => ""),
    exportBackup: vi.fn(async () => new Blob()),
    importBackup: vi.fn(async (_file: Blob, _mode: "merge" | "replace") => {}),
  };
}

let bridge: BridgeMock;
let useAppStore: UseAppStoreModule["useAppStore"];

beforeEach(async () => {
  vi.resetModules();
  vi.clearAllMocks();

  bridge = createBridgeMock();
  vi.doMock("../lib/bridge-provider", () => ({ bridge }));
  vi.doMock("../generated/schemas", async () => ({
    ...(await vi.importActual<typeof import("../generated/schemas")>("../generated/schemas")),
    AppStateSchema: {
      safeParse: (value: unknown) => ({ success: true, data: value }),
    },
  }));

  ({ useAppStore } = await import("./useAppStore"));

  // The backend's Mutate is faithfully emulated: apply the intent to the store's
  // current persisted slice with the same logic the Rust backend runs, return the
  // canonical state. Mirrors the round-trip the real bridge performs.
  bridge.mutate.mockImplementation(async (intent) => {
    const s = useAppStore.getState();
    const prev: AppState = {
      profiles: s.profiles,
      groups: s.groups,
      routingRules: s.routingRules,
      assetFiles: s.assetFiles,
      settings: s.settings,
      activeId: s.activeId,
      version: s.version,
    };
    return applyMutation(prev, intent);
  });
});

describe("useAppStore", () => {
  it("hydrate merges default settings with persisted state", async () => {
    const profile = makeVless({ meta: { id: "p1" } });
    bridge.readState.mockResolvedValue(
      makeState({
        profiles: [profile],
        activeId: profile.meta.id,
        settings: {
          ...DEFAULT_SETTINGS,
          muxXudpConcurrency: undefined,
          muxXudp443: undefined,
        },
      }),
    );
    bridge.status.mockResolvedValue({ ...DEFAULT_STATUS, core: "Xray 25.5.16" });

    await useAppStore.getState().hydrate();

    const state = useAppStore.getState();
    expect(state.hydrated).toBe(true);
    expect(state.profiles[0].meta.id).toBe("p1");
    expect(state.settings.muxXudpConcurrency).toBe(8);
    expect(state.settings.muxXudp443).toBe("reject");
    expect(bridge.onStatus).toHaveBeenCalledTimes(1);
  });

  it("computes upload and download rates from successive service samples", async () => {
    let statusListener: ((status: ServiceStatus) => void) | undefined;
    bridge.onStatus.mockImplementation((cb: (status: ServiceStatus) => void) => {
      statusListener = cb;
      return () => {};
    });
    bridge.status.mockResolvedValue({
      ...DEFAULT_STATUS,
      state: "connected",
      activeId: "p1",
      uploadBytes: 2048,
      downloadBytes: 4096,
    });

    const nowSpy = vi.spyOn(Date, "now");
    nowSpy.mockReturnValueOnce(1_000).mockReturnValueOnce(2_000);

    try {
      await useAppStore.getState().hydrate();
      statusListener?.({
        ...DEFAULT_STATUS,
        state: "connected",
        activeId: "p1",
        uploadBytes: 4096,
        downloadBytes: 8192,
      });
    } finally {
      nowSpy.mockRestore();
    }

    const state = useAppStore.getState();
    expect(state.uploadRate).toBe(2048);
    expect(state.downloadRate).toBe(4096);
  });

  it("hydrate migrates the legacy bypass-lan mode to global", async () => {
    bridge.readState.mockResolvedValue(
      makeState({
        settings: { ...DEFAULT_SETTINGS, routingMode: "bypass-lan" as never },
        assetFiles: [],
      }),
    );

    await useAppStore.getState().hydrate();

    expect(useAppStore.getState().settings.routingMode).toBe("global");
    expect(bridge.mutate).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "replaceState",
        state: expect.objectContaining({
          settings: expect.objectContaining({ routingMode: "global" }),
        }),
      }),
    );
  });

  it("hydrate leaves versioned state intact (no re-migration)", async () => {
    bridge.readState.mockResolvedValue(makeState({ version: "v0.3.2" }));

    await useAppStore.getState().hydrate();

    // The in-memory version is the bundle's own; what matters is that a state
    // already carrying a version is not written back through replaceState.
    expect(useAppStore.getState().version).toBeTruthy();
    expect(bridge.mutate).not.toHaveBeenCalled();
  });

  it("setActive flushes and restarts when service is running", async () => {
    const p1 = makeVless({ meta: { id: "p1", remarks: "One" } });
    const p2 = makeVless({
      meta: { id: "p2", remarks: "Two" },
      uuid: "22222222-2222-2222-2222-222222222222",
    });
    useAppStore.setState({
      profiles: [p1, p2],
      groups: [{ id: "g-main", name: "Main" }],
      settings: DEFAULT_SETTINGS,
      activeId: p1.meta.id,
      service: { ...DEFAULT_STATUS, state: "connected", activeId: p1.meta.id },
    });

    await useAppStore.getState().setActive("p2");

    expect(useAppStore.getState().activeId).toBe("p2");
    expect(bridge.mutate).toHaveBeenCalledWith(expect.objectContaining({ kind: "setActive" }));
    expect(bridge.start).toHaveBeenCalledWith("p2");
  });

  it("setActive keeps the restart cue down while its own restart runs", async () => {
    let statusListener: ((status: ServiceStatus) => void) | undefined;
    bridge.onStatus.mockImplementation((cb: (status: ServiceStatus) => void) => {
      statusListener = cb;
      return () => {};
    });
    const p1 = makeVless({ meta: { id: "p1", remarks: "One" } });
    const p2 = makeVless({ meta: { id: "p2", remarks: "Two" } });
    const running = { ...DEFAULT_STATUS, state: "connected" as const, activeId: "p1" };
    bridge.readState.mockResolvedValue(makeState({ profiles: [p1, p2], activeId: "p1" }));
    bridge.status.mockResolvedValue(running);
    await useAppStore.getState().hydrate();

    // The saved switch is stale against what runs until the start lands; the
    // backend pushes that frame (and keeps ticking it) before the start begins.
    const stale = { ...running, pendingRestart: true };
    bridge.mutate.mockImplementationOnce(async (intent) => {
      statusListener?.(stale);
      return applyMutation(makeState({ profiles: [p1, p2], activeId: "p1" }), intent);
    });
    let resolveStart: ((value: ServiceStatus) => void) | undefined;
    bridge.start.mockImplementationOnce(
      () =>
        new Promise<ServiceStatus>((resolve) => {
          resolveStart = resolve;
        }),
    );

    const switching = useAppStore.getState().setActive("p2");
    await vi.waitFor(() => expect(resolveStart).toBeDefined());
    statusListener?.(stale);
    expect(useAppStore.getState().service.pendingRestart).toBe(false);

    bridge.status.mockResolvedValue({ ...running, activeId: "p2" });
    resolveStart?.({ ...running, activeId: "p2" });
    await switching;
    expect(useAppStore.getState().service.pendingRestart).toBe(false);

    // Outside a start/stop the cue is reported as the backend has it.
    statusListener?.(stale);
    expect(useAppStore.getState().service.pendingRestart).toBe(true);
  });

  it("toggleService exposes connecting state before start resolves", async () => {
    const profile = makeVless({ meta: { id: "p1", remarks: "One" } });
    let resolveStart: ((value: ServiceStatus) => void) | null = null;
    bridge.start.mockImplementation(
      () =>
        new Promise<ServiceStatus>((resolve) => {
          resolveStart = resolve;
        }),
    );
    useAppStore.setState({
      profiles: [profile],
      groups: [{ id: "g-main", name: "Main" }],
      settings: DEFAULT_SETTINGS,
      activeId: profile.meta.id,
      service: DEFAULT_STATUS,
    });

    const pending = useAppStore.getState().toggleService();
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(useAppStore.getState().busy).toBe(true);
    expect(useAppStore.getState().service.state).toBe("connecting");
    expect(useAppStore.getState().service.activeId).toBe(profile.meta.id);

    (resolveStart as unknown as (value: ServiceStatus) => void)({
      ...DEFAULT_STATUS,
      state: "connected",
      activeId: profile.meta.id,
    });
    await pending;
  });

  it("upsertProfile inserts then updates by id", async () => {
    const p1 = makeVless({ meta: { id: "p1", remarks: "One" } });
    const p2 = makeVless({
      meta: { id: "p2", remarks: "Two" },
      uuid: "22222222-2222-2222-2222-222222222222",
    });
    useAppStore.setState({
      profiles: [p1],
      groups: [],
      settings: DEFAULT_SETTINGS,
      activeId: null,
    });

    await useAppStore.getState().upsertProfile(p2);
    expect(useAppStore.getState().profiles.map((p) => p.meta.id)).toEqual(["p2", "p1"]);

    await useAppStore
      .getState()
      .upsertProfile({ ...p2, meta: { ...p2.meta, remarks: "Two updated" } });
    expect(useAppStore.getState().profiles).toHaveLength(2);
    expect(useAppStore.getState().profiles[0].meta.remarks).toBe("Two updated");
  });

  it("cloneProfile copies the profile and leaves the copy untested", async () => {
    const src = makeVless({ meta: { id: "p1", remarks: "Node" } });
    useAppStore.setState({
      profiles: [src],
      groups: [],
      settings: DEFAULT_SETTINGS,
      activeId: null,
      testResults: { p1: { ping: 123, speed: 9_000 } },
    });

    await useAppStore.getState().cloneProfile("p1");

    const copy = useAppStore.getState().profiles.find((p) => p.meta.id !== "p1");
    expect(copy).toBeDefined();
    expect(copy?.meta.remarks).toBe("Node (copy)");
    // ephemeral test results don't carry to the fresh copy id
    expect(useAppStore.getState().testResults[copy?.meta.id ?? ""]).toBeUndefined();
  });

  it("ping/realPing/speed results land in the ephemeral testResults map, not the profile", async () => {
    const p = makeVless({ meta: { id: "p1" } });
    useAppStore.setState({
      profiles: [p],
      groups: [],
      settings: DEFAULT_SETTINGS,
      activeId: null,
      testResults: {},
    });

    bridge.ping.mockResolvedValueOnce(88);
    await useAppStore.getState().testProfile("p1", "tcpPing");
    expect(useAppStore.getState().testResults.p1.ping).toBe(88);
    // the persisted profile is never touched by a test
    const meta = useAppStore.getState().profiles[0].meta as Record<string, unknown>;
    expect(meta.ping).toBeUndefined();

    bridge.realPing.mockResolvedValueOnce(150);
    await useAppStore.getState().testProfile("p1", "realPing");
    expect(useAppStore.getState().testResults.p1.ping).toBe(150);

    bridge.speedTest.mockResolvedValueOnce(1_500_000);
    await useAppStore.getState().testProfile("p1", "speed");
    expect(useAppStore.getState().testResults.p1.speed).toBe(1_500_000);
  });

  it("removeProfile stops service and clears active id when removing active profile", async () => {
    const active = makeVless({ meta: { id: "p1" } });
    const other = makeVless({
      meta: { id: "p2" },
      uuid: "22222222-2222-2222-2222-222222222222",
    });
    useAppStore.setState({
      profiles: [active, other],
      groups: [{ id: "g-main", name: "Main" }],
      settings: DEFAULT_SETTINGS,
      activeId: active.meta.id,
      service: { ...DEFAULT_STATUS, state: "connected", activeId: active.meta.id },
    });

    await useAppStore.getState().removeProfile(active.meta.id);

    expect(bridge.stop).toHaveBeenCalled();
    expect(useAppStore.getState().activeId).toBeNull();
    expect(useAppStore.getState().profiles.map((p) => p.meta.id)).toEqual(["p2"]);
  });

  it("removeAssetFile removes the asset without touching routing settings", async () => {
    const geoip = makeAsset({ id: "asset-geoip" });
    useAppStore.setState({
      profiles: [],
      groups: [{ id: "g-main", name: "Main" }],
      assetFiles: [geoip],
      settings: { ...DEFAULT_SETTINGS, routingMode: "rules" },
      activeId: null,
    });

    await useAppStore.getState().removeAssetFile(geoip.id);

    expect(useAppStore.getState().assetFiles).toEqual([]);
    expect(useAppStore.getState().settings.routingMode).toBe("rules");
  });

  it("daemon assetsUpdated push reloads assets and reports the restart", async () => {
    let push: ((info: AssetsUpdatedEvent) => void) | null = null;
    bridge.onAssetsUpdated.mockImplementation((cb) => {
      push = cb;
      return () => {};
    });
    await useAppStore.getState().hydrate();
    expect(push).not.toBeNull();

    // The daemon refreshed the geo assets headlessly and stamped lastUpdated.
    const asset = makeAsset({ id: "a1", remarks: "geoip.dat", lastUpdated: 1234 });
    bridge.readState.mockResolvedValue(makeState({ assetFiles: [asset] }));

    (push as unknown as (info: AssetsUpdatedEvent) => void)({
      remarks: ["geoip.dat"],
      restarted: true,
    });
    await vi.waitFor(() => {
      expect(useAppStore.getState().assetFiles[0]?.lastUpdated).toBe(1234);
    });

    // Newest first: the unrequested restart, then the asset that caused it.
    const feed = useAppStore.getState().recentActivity;
    expect(feed[0].icon).toBe("autorenew");
    expect(feed[1].text).toContain("geoip.dat");
  });

  describe("recentActivity", () => {
    it("starts empty", async () => {
      await useAppStore.getState().hydrate();
      expect(useAppStore.getState().recentActivity).toHaveLength(0);
    });

    it("toggleService start pushes serviceStarted activity", async () => {
      const profile = makeVless({ meta: { id: "p1", remarks: "MyNode" } });
      bridge.readState.mockResolvedValue(
        makeState({ profiles: [profile], activeId: profile.meta.id }),
      );
      await useAppStore.getState().hydrate();

      await useAppStore.getState().toggleService();

      const feed = useAppStore.getState().recentActivity;
      expect(feed).toHaveLength(1);
      expect(feed[0].icon).toBe("play_circle");
      expect(feed[0].text).toContain("MyNode");
      expect(feed[0].color).toBe("var(--running)");
      expect(feed[0].at).toBeGreaterThan(0);
    });

    it("toggleService stop pushes serviceStopped activity", async () => {
      const profile = makeVless({ meta: { id: "p1", remarks: "MyNode" } });
      bridge.readState.mockResolvedValue(
        makeState({ profiles: [profile], activeId: profile.meta.id }),
      );
      bridge.status.mockResolvedValue({
        ...DEFAULT_STATUS,
        state: "connected",
        activeId: profile.meta.id,
      });
      await useAppStore.getState().hydrate();

      await useAppStore.getState().toggleService();

      const feed = useAppStore.getState().recentActivity;
      expect(feed[0].icon).toBe("stop_circle");
      expect(feed[0].color).toBe("var(--error)");
    });

    it("addProfiles pushes profileImported activity", async () => {
      await useAppStore.getState().hydrate();
      const profiles = [makeVless({ meta: { id: "p1" } }), makeVless({ meta: { id: "p2" } })];
      await useAppStore.getState().addProfiles(profiles);

      const feed = useAppStore.getState().recentActivity;
      expect(feed).toHaveLength(1);
      expect(feed[0].icon).toBe("download");
      expect(feed[0].text).toMatch(/2/);
    });
  });
});
