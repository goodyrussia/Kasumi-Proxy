import { Card, ListRow, SectionLabel } from "../../../components";
import { useT } from "../../../i18n";
import { useAppStore } from "../../../store/useAppStore";

/** Module identity: name, version (baked into the bundle at build), the MIT
 *  license and the bundled Xray core. The module is updated by the root manager,
 *  so there is no in-app updater. */
export function AboutSection() {
  const t = useT();
  const version = useAppStore((s) => s.version);
  const caps = useAppStore((s) => s.caps);

  return (
    <>
      <SectionLabel>{t("settings.page.about")}</SectionLabel>
      <Card style={{ padding: "4px 14px" }}>
        <ListRow icon="info" title={t("overview.title")} sub="kasumi-proxy" />
        <ListRow icon="tag" title={t("settings.appVersion")} sub={version} />
        <ListRow icon="bolt" title={t("settings.xrayVersion")} sub={caps?.xrayVersion || "—"} />
        <ListRow icon="description" title={t("settings.license")} sub="MIT" />
      </Card>
    </>
  );
}
