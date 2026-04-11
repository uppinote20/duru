class Duru < Formula
  desc "Terminal dashboard for Claude Code — explore, manage, and monitor your setup"
  homepage "https://github.com/uppinote20/duru"
  version "{{VERSION}}"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/uppinote20/duru/releases/download/v{{VERSION}}/duru-aarch64-apple-darwin.tar.gz"
      sha256 "{{SHA256_AARCH64_APPLE_DARWIN}}"
    else
      url "https://github.com/uppinote20/duru/releases/download/v{{VERSION}}/duru-x86_64-apple-darwin.tar.gz"
      sha256 "{{SHA256_X86_64_APPLE_DARWIN}}"
    end
  end

  on_linux do
    url "https://github.com/uppinote20/duru/releases/download/v{{VERSION}}/duru-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "{{SHA256_X86_64_UNKNOWN_LINUX_GNU}}"
  end

  def install
    bin.install "duru"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/duru --version")
  end
end
