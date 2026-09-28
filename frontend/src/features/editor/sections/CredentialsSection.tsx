import { Field, Segmented, Select, SettingRow, Switch } from "../../../components";
import type { Profile } from "../../../generated/bindings";
import {
  FLOW_OPTS,
  PACKET_ENCODING_OPTS,
  SS_METHOD_OPTS,
  VMESS_ENC_OPTS,
} from "../../../generated/defaults";
import { useT } from "../../../i18n";
import type { FieldErrors, RootSetter } from "../types";

const reservedToText = (reserved?: number[]) => (reserved ?? []).join(", ");
const textToReserved = (s: string) =>
  s
    .split(/[\s,]+/)
    .filter(Boolean)
    .map(Number)
    .filter((n) => Number.isFinite(n));

export function CredentialsSection({
  draft,
  setRoot,
  errors,
}: {
  draft: Profile;
  setRoot: RootSetter;
  errors: FieldErrors;
}) {
  const t = useT();

  return (
    <>
      {(draft.protocol === "vless" || draft.protocol === "vmess") && (
        <Field
          label={t("editor.userId")}
          value={draft.uuid ?? ""}
          onChange={(value) => setRoot({ uuid: value })}
          error={errors.uuid}
        />
      )}
      {draft.protocol === "vless" && (
        <>
          <Select
            label={t("editor.flow")}
            value={draft.flow ?? ""}
            options={FLOW_OPTS}
            onChange={(value) => setRoot({ flow: value })}
          />
          <Segmented
            label={t("editor.packetEncoding")}
            value={draft.packetEncoding ?? ""}
            options={PACKET_ENCODING_OPTS}
            onChange={(value) => setRoot({ packetEncoding: value })}
          />
          <Field
            label={t("editor.encryption")}
            value={draft.encryption ?? "none"}
            onChange={(value) => setRoot({ encryption: value })}
          />
        </>
      )}
      {draft.protocol === "vmess" && (
        <>
          <div className="input-row" style={{ marginBottom: 14 }}>
            <Select
              label={t("editor.encryption")}
              value={draft.encryption ?? "auto"}
              options={VMESS_ENC_OPTS}
              onChange={(value) => setRoot({ encryption: value })}
            />
            <div style={{ width: 96, flex: "0 0 auto" }}>
              <Field
                label={t("editor.alterId")}
                type="number"
                value={draft.alterId ?? 0}
                onChange={(value) => setRoot({ alterId: Number(value) })}
              />
            </div>
          </div>
          <Segmented
            label={t("editor.packetEncoding")}
            value={draft.packetEncoding ?? ""}
            options={PACKET_ENCODING_OPTS}
            onChange={(value) => setRoot({ packetEncoding: value })}
          />
          <SettingRow title={t("editor.vmessGlobalPadding")}>
            <Switch
              on={!!draft.vmessGlobalPadding}
              onChange={(value) => setRoot({ vmessGlobalPadding: value })}
            />
          </SettingRow>
          <SettingRow title={t("editor.vmessAuthenticatedLength")}>
            <Switch
              on={!!draft.vmessAuthenticatedLength}
              onChange={(value) => setRoot({ vmessAuthenticatedLength: value })}
            />
          </SettingRow>
        </>
      )}
      {draft.protocol === "trojan" && (
        <>
          <Field
            label={t("editor.password")}
            value={draft.password ?? ""}
            onChange={(value) => setRoot({ password: value })}
            error={errors.password}
          />
          <Select
            label={t("editor.flow")}
            value={draft.flow ?? ""}
            options={FLOW_OPTS}
            onChange={(value) => setRoot({ flow: value })}
          />
        </>
      )}
      {draft.protocol === "shadowsocks" && (
        <>
          <Field
            label={t("editor.password")}
            value={draft.password ?? ""}
            onChange={(value) => setRoot({ password: value })}
            error={errors.password}
          />
          <Select
            label={t("editor.method")}
            value={draft.method ?? "aes-256-gcm"}
            options={SS_METHOD_OPTS}
            onChange={(value) => setRoot({ method: value })}
          />
        </>
      )}
      {(draft.protocol === "socks" || draft.protocol === "http") && (
        <div className="input-row" style={{ marginBottom: 14 }}>
          <Field
            label={t("editor.username")}
            mono={false}
            value={draft.username ?? ""}
            onChange={(value) => setRoot({ username: value })}
          />
          <Field
            label={t("editor.password")}
            value={draft.password ?? ""}
            onChange={(value) => setRoot({ password: value })}
          />
        </div>
      )}

      {draft.protocol === "wireguard" && (
        <>
          <Field
            label={t("editor.privateKey")}
            value={draft.secretKey ?? ""}
            onChange={(value) => setRoot({ secretKey: value })}
            error={errors.secretKey}
          />
          <Field
            label={t("editor.peerPublicKey")}
            value={draft.peerPublicKey ?? ""}
            onChange={(value) => setRoot({ peerPublicKey: value })}
            error={errors.peerPublicKey}
          />
          <Field
            label={t("editor.preSharedKey")}
            value={draft.preSharedKey ?? ""}
            onChange={(value) => setRoot({ preSharedKey: value })}
          />
          <div className="input-row" style={{ marginBottom: 14 }}>
            <Field
              label={t("editor.localAddress")}
              mono={false}
              value={draft.localAddress ?? ""}
              onChange={(value) => setRoot({ localAddress: value })}
            />
            <div style={{ width: 110, flex: "0 0 auto" }}>
              <Field
                label={t("editor.reserved")}
                value={reservedToText(draft.reserved)}
                onChange={(value) => setRoot({ reserved: textToReserved(value) })}
                hint={t("editor.reservedHint")}
              />
            </div>
          </div>
          <div className="input-row" style={{ marginBottom: 14 }}>
            <Field
              label={t("editor.mtu")}
              type="number"
              value={draft.mtu ?? 1420}
              onChange={(value) => setRoot({ mtu: Number(value) })}
            />
            <Field
              label={t("editor.wgWorkers")}
              type="number"
              value={draft.workers ?? 0}
              onChange={(value) => setRoot({ workers: Number(value) || 0 })}
            />
          </div>
          <Field
            label={t("editor.wgPersistentKeepalive")}
            type="number"
            value={draft.persistentKeepalive ?? 0}
            onChange={(value) => setRoot({ persistentKeepalive: Number(value) || 0 })}
          />
        </>
      )}
    </>
  );
}
