import {
  blurOnWheel,
  Card,
  Disclosure,
  Field,
  SectionLabel,
  Segmented,
  Select,
  SettingRow,
  Switch,
} from "../../../components";
import type { AdvancedSettings_Serialize, TunEngine } from "../../../generated/bindings";
import {
  TUN_ENGINE_OPTS,
  TUN_KNOBS_BY_ENGINE,
  type TunKnobSpec,
} from "../../../generated/defaults";
import { type DictKey, useT } from "../../../i18n";
import type { AdvancedSettings } from "../../../lib/bridge";

// Display labels for the TUN engines. Presentation only; the selectable engines
// and their settings come from the generated `TUN_ENGINE_OPTS`/`TUN_KNOBS_BY_ENGINE`
// (single-sourced from Rust), so a new engine variant surfaces here as a
// missing-key type error rather than silent omission.
const ENGINE_LABEL: Record<TunEngine, string> = {
  tun2socks: "tun2socks",
  hev: "hev-socks5-tunnel",
};

// Labels for engine settings fields. Which fields an engine reads, and how each is
// edited, comes from Rust (`TUN_KNOBS_BY_ENGINE`); this only names them. A field
// without a label here still renders, under its raw name.
const KNOB_LABEL: Partial<Record<keyof AdvancedSettings_Serialize, DictKey>> = {
  tunConnectTimeoutMs: "settings.tunConnectTimeout",
  tunTcpRwTimeoutMs: "settings.tunTcpRwTimeout",
  tunUdpRwTimeoutMs: "settings.tunUdpRwTimeout",
  tunTcpBufferSize: "settings.tunTcpBuffer",
  tunUdpRecvBufferSize: "settings.tunUdpRecvBuffer",
};

export function TunEngineSection({
  settings,
  set,
}: {
  settings: AdvancedSettings;
  set: <K extends keyof AdvancedSettings>(key: K, value: AdvancedSettings[K]) => void;
}) {
  const t = useT();

  const engine = settings.tunEngine;
  const knobs: TunKnobSpec[] = TUN_KNOBS_BY_ENGINE[engine] ?? [];
  const excludeCount = (settings.tunExcludeAddresses ?? "").split(/[\s,]+/).filter(Boolean).length;

  return (
    <>
      <SectionLabel>{t("settings.tunEngine")}</SectionLabel>
      <Card style={{ padding: "4px 14px" }}>
        <SettingRow title={t("settings.tunEngine")}>
          <Select
            style={{ width: 170 }}
            value={engine}
            onChange={(v) => set("tunEngine", v)}
            options={TUN_ENGINE_OPTS.map((e) => ({ value: e, label: ENGINE_LABEL[e] }))}
          />
        </SettingRow>
        <div className="hint" style={{ padding: "4px 0 10px" }}>
          {t("settings.tunEngineHint")}
        </div>
        {knobs.length > 0 && (
          <div style={{ paddingBottom: 6 }}>
            <div className="field-label" style={{ margin: "4px 0 0" }}>
              {t("settings.tunEngineSettings")}
            </div>
            {knobs.map((spec) => (
              <TunKnobRow key={spec.field} spec={spec} settings={settings} set={set} />
            ))}
          </div>
        )}
      </Card>

      <Card style={{ padding: "4px 14px", marginTop: 12 }}>
        <SettingRow title={t("settings.strictRoute")} hint={t("settings.strictRouteSub")}>
          <Switch on={settings.strictRoute} onChange={(value) => set("strictRoute", value)} />
        </SettingRow>
        <SettingRow title={t("settings.tunMtu")}>
          <input
            className="input compact"
            type="number"
            inputMode="numeric"
            aria-label={t("settings.tunMtu")}
            value={settings.tunMtu}
            onWheel={blurOnWheel}
            onChange={(e) => set("tunMtu", Number(e.target.value))}
          />
        </SettingRow>
        <div className="setting-row" style={{ display: "block", padding: 0 }}>
          <Disclosure
            label={
              excludeCount > 0
                ? `${t("settings.tunExclude")} · ${excludeCount}`
                : t("settings.tunExclude")
            }
          >
            <Field
              area
              value={settings.tunExcludeAddresses ?? ""}
              placeholder={t("settings.tunExcludePh")}
              hint={t("settings.tunExcludeHint")}
              onChange={(value) => set("tunExcludeAddresses", value)}
            />
          </Disclosure>
        </div>
      </Card>
    </>
  );
}

/** One engine setting, rendered from its Rust-side description. */
function TunKnobRow({
  spec,
  settings,
  set,
}: {
  spec: TunKnobSpec;
  settings: AdvancedSettings;
  set: <K extends keyof AdvancedSettings>(key: K, value: AdvancedSettings[K]) => void;
}) {
  const t = useT();
  const labelKey = KNOB_LABEL[spec.field];
  const label = labelKey ? t(labelKey) : spec.field;
  // The spec says what kind of value the field holds; the settings type can't
  // narrow on a runtime field name, so read and write through a plain record.
  const current = (settings as Record<string, unknown>)[spec.field];
  const write = (value: string | number) =>
    set(spec.field as keyof AdvancedSettings, value as never);

  return (
    <SettingRow title={label}>
      {spec.kind === "choice" ? (
        <Segmented
          size="sm"
          ariaLabel={label}
          value={String(current ?? "")}
          onChange={write}
          options={spec.options.map((o) => ({ value: o, label: o }))}
        />
      ) : (
        <input
          className="input compact"
          type="number"
          inputMode="numeric"
          aria-label={label}
          value={typeof current === "number" ? current : ""}
          onWheel={blurOnWheel}
          onChange={(e) => write(Number(e.target.value))}
        />
      )}
    </SettingRow>
  );
}
