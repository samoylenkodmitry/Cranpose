#!/usr/bin/env python3
"""Signs an assembled .app for an iPhone, as Xcode's automatic signing would.

  sign.py <Everbloom.app> --platform iphoneos --identity <identity> --profile <file.mobileprovision>

Adds the SDK keys Xcode writes into Info.plist, embeds the provisioning
profile and signs the bundle with the profile's entitlements, where a wildcard
application identifier becomes the bundle's own.
"""
import argparse
import plistlib
import re
import subprocess
import tempfile
from pathlib import Path


def output(*command: str) -> str:
    return subprocess.check_output(command, text=True).strip()


def sdk_keys(sdk: str) -> dict:
    version = output("xcrun", "--sdk", sdk, "--show-sdk-version")
    build = output("xcrun", "--sdk", sdk, "--show-sdk-build-version")
    xcode = output("xcodebuild", "-version").splitlines()
    major, minor, patch = (re.match(r"Xcode (\S+)", xcode[0]).group(1).split(".") + ["0", "0"])[:3]
    platform = {"iphoneos": "iPhoneOS", "iphonesimulator": "iPhoneSimulator"}[sdk]
    return {
        "DTPlatformName": sdk,
        "DTPlatformVersion": version,
        "DTPlatformBuild": build,
        "DTSDKName": f"{sdk}{version}",
        "DTSDKBuild": build,
        "DTXcode": f"{int(major):02d}{int(minor)}{int(patch)}",
        "DTXcodeBuild": xcode[1].split()[-1],
        "DTCompiler": "com.apple.compilers.llvm.clang.1_0",
        "BuildMachineOSBuild": output("sw_vers", "-buildVersion"),
        "CFBundleSupportedPlatforms": [platform],
    }


def profile_entitlements(profile: Path, bundle_id: str) -> dict:
    decoded = subprocess.check_output(["security", "cms", "-D", "-i", str(profile)])
    entitlements = plistlib.loads(decoded)["Entitlements"]

    def resolve(value):
        if isinstance(value, str) and value.endswith(".*"):
            return value[:-1] + bundle_id
        if isinstance(value, list):
            return [resolve(item) for item in value]
        return value

    return {key: resolve(value) for key, value in entitlements.items()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("app", type=Path)
    parser.add_argument("--platform", default="iphoneos", choices=["iphoneos", "iphonesimulator"])
    parser.add_argument("--identity", required=True)
    parser.add_argument("--profile", type=Path, required=True)
    args = parser.parse_args()

    info_path = args.app / "Info.plist"
    info = plistlib.loads(info_path.read_bytes())
    info.update(sdk_keys(args.platform))
    info_path.write_bytes(plistlib.dumps(info))
    (args.app / "embedded.mobileprovision").write_bytes(args.profile.read_bytes())
    with tempfile.TemporaryDirectory() as work:
        entitlements = Path(work) / "app.entitlements"
        entitlements.write_bytes(plistlib.dumps(profile_entitlements(args.profile, info["CFBundleIdentifier"])))
        subprocess.run(
            ["codesign", "--force", "--sign", args.identity, "--entitlements", str(entitlements),
             "--generate-entitlement-der", str(args.app)],
            check=True,
        )
    subprocess.run(["codesign", "--verify", "--strict", str(args.app)], check=True)


if __name__ == "__main__":
    main()
