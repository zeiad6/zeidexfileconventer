cask "zeidex-convert" do
  arch arm: "aarch64", intel: "x64"

  version "__VERSION__"
  sha256 arm:   "__MAC_ARM64_SHA256__",
         intel: "__MAC_X64_SHA256__"

  url "https://github.com/zeiad6/zeidexfileconventer/releases/download/v#{version}/Zeidex.Convert_#{version}_#{arch}.dmg",
      verified: "github.com/zeiad6/zeidexfileconventer/"
  name "Zeidex Convert"
  desc "Offline-first file converter and media processor with Shift-drag wheel"
  homepage "https://zeidexsoft.com/"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :big_sur"

  app "Zeidex Convert.app"

  # Removes Gatekeeper quarantine attributes on unsigned builds; once Apple Developer ID
  # signing + notarytool stapling is configured in CI, this hook becomes a harmless no-op.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-cr", "#{appdir}/Zeidex Convert.app"],
                   sudo: false
  end

  zap trash: [
    "~/Library/Application Support/ai.storyteller.craft-anything",
    "~/Library/Application Support/com.zeidexsoft.convert",
    "~/Library/Preferences/com.zeidexsoft.convert.plist",
  ]
end
