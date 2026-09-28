import {
  Btn,
  Card,
  Chip,
  Disclosure,
  Field,
  IconBtn,
  ListRow,
  NavRow,
  RowToggle,
  Segmented,
  SettingRow,
  Switch,
} from "../../../components";
import type { RoutingRule } from "../../../generated/bindings";
import { useFormatters, useT } from "../../../i18n";
import type { AdvancedSettings } from "../../../lib/bridge";
import { isCatchAllRule, isRedundantCatchAll, ruleIcon, ruleSummary } from "../helpers";
import { makePresetRule, RULE_PRESETS } from "../rule-presets";

export function RoutingSection({
  settings,
  set,
  routingRules,
  profiles,
  setRoutingMode,
  openNewRule,
  onEditRule,
  addRoutingRule,
  updateRoutingRule,
  reorderRoutingRules,
  removeRoutingRule,
  onOpenRulesIO,
  onOpenAppFilter,
}: {
  settings: AdvancedSettings;
  set: <K extends keyof AdvancedSettings>(key: K, value: AdvancedSettings[K]) => void;
  routingRules: RoutingRule[];
  profiles: { id: string; remarks: string }[];
  setRoutingMode: (mode: AdvancedSettings["routingMode"]) => void;
  openNewRule: () => void;
  onEditRule: (rule: RoutingRule) => void;
  addRoutingRule: (rule: RoutingRule) => void;
  updateRoutingRule: (id: string, patch: Partial<RoutingRule>) => void;
  reorderRoutingRules: (from: number, to: number) => void;
  removeRoutingRule: (id: string) => void;
  onOpenRulesIO: () => void;
  onOpenAppFilter: () => void;
}) {
  const t = useT();
  const formatters = useFormatters();
  const profileName = (tag: string) => profiles.find((p) => p.id === tag)?.remarks;
  const catchAllIndex = routingRules.findIndex(isCatchAllRule);
  const catchAllRedundant = isRedundantCatchAll(routingRules, catchAllIndex);
  const appFilterCount = Object.keys(settings.appFilter ?? {}).length;
  const appFilterSummary =
    appFilterCount > 0
      ? t("appFilter.subtitle", { n: appFilterCount })
      : settings.appCaptureMode === "none"
        ? t("appFilter.captureNone")
        : t("appFilter.captureAll");

  return (
    <>
      <Card style={{ padding: "4px 14px", marginTop: 8 }}>
        <SettingRow stacked title={t("settings.routingMode")}>
          <Segmented
            ariaLabel={t("settings.routingMode")}
            value={settings.routingMode}
            onChange={setRoutingMode}
            options={[
              { value: "global", label: t("settings.routingGlobal") },
              { value: "custom", label: t("settings.routingCustom") },
              { value: "rules", label: t("settings.routingRulesEditor") },
            ]}
          />
        </SettingRow>
        {settings.routingMode === "custom" && (
          <div style={{ padding: "8px 0 4px" }}>
            <Field
              area
              label={t("settings.customRouting")}
              value={settings.customRouting ?? ""}
              placeholder={t("settings.customRoutingPh")}
              hint={t("settings.customRoutingHint")}
              onChange={(value) => set("customRouting", value)}
            />
          </div>
        )}
        {settings.routingMode === "rules" && (
          <div style={{ padding: "12px 0 8px" }}>
            <div className="hint" style={{ marginBottom: 10 }}>
              {t("settings.routingRulesHint")}
            </div>
            <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginBottom: 12 }}>
              <Btn variant="tonal" sm icon="add" onClick={openNewRule}>
                {t("settings.routingAddRule")}
              </Btn>
              <Btn variant="outline" sm icon="swap_vert" onClick={onOpenRulesIO}>
                {t("settings.routingImportExport")}
              </Btn>
            </div>
            <div style={{ marginBottom: 14 }}>
              <div className="hint" style={{ marginBottom: 6 }}>
                {t("settings.rulePresets")}
              </div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                {RULE_PRESETS.map((preset) => (
                  <Chip
                    key={preset.id}
                    icon={preset.icon}
                    onClick={() => addRoutingRule(makePresetRule(preset, t(preset.labelKey)))}
                  >
                    {t(preset.labelKey)}
                  </Chip>
                ))}
              </div>
            </div>
            {routingRules.length === 0 ? (
              <div className="hint" style={{ padding: "4px 0 8px" }}>
                {t("settings.routingEmpty")}
              </div>
            ) : (
              routingRules.map((rule, index) => (
                <ListRow
                  key={rule.id}
                  icon={ruleIcon(rule)}
                  title={rule.remarks || t("settings.routingRuleDefault", { n: index + 1 })}
                  sub={
                    <>
                      {ruleSummary(rule, t, formatters, profileName)}
                      {index === catchAllIndex && (
                        <div style={{ color: "var(--warn)", marginTop: 2 }}>
                          {t(
                            catchAllRedundant
                              ? "settings.routingCatchAllRedundant"
                              : "settings.routingCatchAll",
                          )}
                        </div>
                      )}
                      {catchAllIndex >= 0 && index > catchAllIndex && rule.enabled && (
                        <div style={{ color: "var(--on-surface-faint)", marginTop: 2 }}>
                          {t("settings.routingUnreachable")}
                        </div>
                      )}
                    </>
                  }
                  onClick={() => onEditRule(rule)}
                  right={
                    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                      <IconBtn
                        name="arrow_upward"
                        sm
                        title={t("settings.routingMoveUp")}
                        onClick={() => reorderRoutingRules(index, index - 1)}
                        style={index === 0 ? { opacity: 0.4 } : undefined}
                      />
                      <IconBtn
                        name="arrow_downward"
                        sm
                        title={t("settings.routingMoveDown")}
                        onClick={() => reorderRoutingRules(index, index + 1)}
                        style={index === routingRules.length - 1 ? { opacity: 0.4 } : undefined}
                      />
                      <Switch
                        on={rule.enabled}
                        onChange={(value) => updateRoutingRule(rule.id, { enabled: value })}
                      />
                      <IconBtn
                        name="delete"
                        sm
                        title={t("settings.routingDelete")}
                        onClick={() => removeRoutingRule(rule.id)}
                      />
                    </div>
                  }
                />
              ))
            )}
          </div>
        )}
      </Card>

      <Card style={{ padding: "4px 14px", marginTop: 12 }}>
        <NavRow
          icon="smart_toy"
          title={t("appFilter.openPage")}
          sub={`${t("appFilter.openPageSub")} · ${appFilterSummary}`}
          onClick={onOpenAppFilter}
        />
      </Card>

      <Card style={{ padding: "0 14px", marginTop: 12 }}>
        <Disclosure label={t("settings.advanced")}>
          <SettingRow stacked title={t("settings.domainStrategy4Xray")}>
            <Segmented
              ariaLabel={t("settings.domainStrategy4Xray")}
              value={settings.domainStrategy}
              onChange={(value) => set("domainStrategy", value)}
              options={[
                { value: "AsIs", label: t("settings.domainStrategy4Xray.AsIs") },
                { value: "IPIfNonMatch", label: t("settings.domainStrategy4Xray.IPIfNonMatch") },
                { value: "IPOnDemand", label: t("settings.domainStrategy4Xray.IPOnDemand") },
              ]}
            />
          </SettingRow>
          <RowToggle
            icon="travel_explore"
            title={t("settings.domainSniffing")}
            sub={t("settings.domainSniffingSub")}
            on={settings.domainSniffing}
            onChange={(value) => set("domainSniffing", value)}
          />
          <RowToggle
            icon="route"
            title={t("settings.routeOnly")}
            sub={t("settings.routeOnlySub")}
            on={settings.routeOnly}
            onChange={(value) => set("routeOnly", value)}
          />
        </Disclosure>
      </Card>
    </>
  );
}
