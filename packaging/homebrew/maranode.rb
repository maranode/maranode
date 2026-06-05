class Maranode < Formula
  desc "Private, air-gapped AI inference runtime"
  homepage "https://github.com/maranode/maranode"
  version "VERSION"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/maranode/maranode/releases/download/vVERSION/maranode-vVERSION-aarch64-apple-darwin-metal.tar.gz"
      sha256 "SHA256_ARM64_METAL"
    else
      url "https://github.com/maranode/maranode/releases/download/vVERSION/maranode-vVERSION-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_X86_64"
    end
  end

  def install
    bin.install "maranoded"
    bin.install "maranode"
  end

  service do
    run          [opt_bin/"maranoded"]
    keep_alive   true
    log_path     var/"log/maranode/maranoded.log"
    error_log_path var/"log/maranode/maranoded.log"
    working_dir  var/"lib/maranode"
  end

  def post_install
    (var/"lib/maranode").mkpath
    (var/"log/maranode").mkpath
  end

  test do
    assert_predicate bin/"maranode",   :exist?
    assert_predicate bin/"maranoded",  :exist?
    assert_match version.to_s, shell_output("#{bin}/maranode --version")
  end
end
