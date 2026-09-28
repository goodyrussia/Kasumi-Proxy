import { Card, ListRow, SectionLabel } from "../../../components";
import { useT } from "../../../i18n";

export function DiagnosticsSection({
  bridgeMode,
  xrayVersion,
  tun,
  profilesCount,
  activeId,
}: {
  bridgeMode: string;
  xrayVersion: string;
  tun: boolean;
  profilesCount: number;
  activeId: string | null;
}) {
  const t = useT();
  const notInstalled = t("common.notInstalled");

  return (
    <>
      <SectionLabel>{t("settings.diagnostics")}</SectionLabel>
      <Card style={{ padding: "4px 14px" }}>
        <ListRow icon="link" title={t("settings.bridge")} sub={bridgeMode} />
        <ListRow icon="bolt" title={t("settings.xrayVersion")} sub={xrayVersion || notInstalled} />
        <ListRow
          icon="shield_moon"
          title={t("settings.tun")}
          sub={tun ? t("common.available") : t("common.unavailable")}
        />
        <ListRow icon="dns" title={t("settings.profiles")} sub={`${profilesCount}`} />
        <ListRow
          icon="bookmark"
          title={t("settings.activeProfile")}
          sub={activeId ? t("settings.activeSelected") : t("settings.activeNone")}
        />
      </Card>
    </>
  );
}
