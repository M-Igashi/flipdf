class Flipdf < Formula
  desc "Merge duplex-scanned PDFs into proper page order"
  homepage "https://github.com/M-Igashi/flipdf"
  version "0.1.0"
  license "MIT"

  on_macos do
    url "https://github.com/M-Igashi/flipdf/releases/download/v#{version}/flipdf-v#{version}-macos.tar.gz"
    # sha256 "UPDATE_WITH_ACTUAL_SHA256"
  end

  depends_on "qpdf"

  def install
    bin.install "flipdf"
  end

  test do
    assert_match "flipdf", shell_output("#{bin}/flipdf --version")
  end
end
