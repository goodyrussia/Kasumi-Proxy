#!/system/bin/sh
# shellcheck disable=SC2034 # Magisk installer reads this flag from the script environment.
SKIPUNZIP=1

mkdir -p "$MODPATH/bin"
mkdir -p "$MODPATH/webroot"

ui_print "- Detected Architecture: $ARCH"

# 1. Extract the arm64 payload directly into the module's private bin/ dir. The
#    payload bundles the Xray core, the two TUN engines (tun2socks,
#    hev-socks5-tunnel) and the kasumi-proxy daemon. arm64 only.
case "$ARCH" in
arm64)
	ui_print "- Unpacking xray + tun helpers + daemon for arm64-v8a..."
	unzip -j -o "$ZIPFILE" "bin/arm64-v8a/*" -d "$MODPATH/bin"
	;;
*)
	ui_print "❌ Unsupported CPU architecture: $ARCH"
	abort "Kasumi Proxy ships arm64-v8a only."
	;;
esac

# 2. Extract the rest of the payload, preserving its layout. The bin/ dir is
#    skipped — it was flattened into $MODPATH/bin above — as is the installer's
#    own META-INF/. Everything else (scripts, webroot, …) ships as-is, so adding
#    a payload file needs no change here.
ui_print "- Extracting management scripts and Webroot components..."
unzip -o "$ZIPFILE" -x "bin/arm64-v8a/*" "META-INF/*" "customize.sh" -d "$MODPATH/"

# 3. Enforce strict executable permissions natively
ui_print "- Setting executable permissions..."
chmod 755 "$MODPATH/bin/"*

ui_print "- Setup /data/adb/kasumi-proxy directory"
# Keep any existing state across re-installs/upgrades; only create when absent.
mkdir -p "/data/adb/kasumi-proxy"

ui_print "Kasumi Proxy configuration deployment complete!"

# Grant camera permission to root manager apps so QR scanner works
ui_print "- Granting camera permission to root manager..."
for pkg in \
	com.topjohnwu.magisk \
	me.weishu.kernelsu \
	me.bmax.superuser \
	com.rifsxd.kasuinext \
	io.github.huskydg.magisk \
	com.apatch.apm; do
	if pm list packages 2>/dev/null | grep -qF "$pkg"; then
		pm grant "$pkg" android.permission.CAMERA 2>/dev/null
		ui_print "  granted CAMERA -> $pkg"
	fi
done
