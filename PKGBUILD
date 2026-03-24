pkgname=razer-coolingpad-linux
pkgver=0.1.0
pkgrel=1
pkgdesc='Fan curve controller for the Razer Laptop Cooling Pad'
arch=('any')
url='https://github.com/Nox1con/razer-coolingpad-linux'
license=('MIT')
depends=('python' 'python-hidapi' 'systemd')
install="$pkgname.install"
backup=('etc/razer-coolingpad-linux/config.json')
source=(
  'LICENSE'
  'README.md'
  'config.json'
  'razer-coolingpad-fancurve.py'
  'razer-coolingpad-preset'
  'razer-coolingpad-linux.install'
  'razer-coolingpad.service'
)
sha256sums=(
  'SKIP'
  'SKIP'
  'SKIP'
  'SKIP'
  'SKIP'
  'SKIP'
  'SKIP'
)

package() {
  install -Dm644 "$srcdir/LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 "$srcdir/README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"

  install -Dm755 "$srcdir/razer-coolingpad-fancurve.py" \
    "$pkgdir/usr/bin/razer-coolingpad-fancurve"
  install -Dm755 "$srcdir/razer-coolingpad-preset" \
    "$pkgdir/usr/bin/razer-coolingpad-preset"
  install -Dm644 "$srcdir/razer-coolingpad.service" \
    "$pkgdir/usr/lib/systemd/system/razer-coolingpad.service"
  install -Dm644 "$srcdir/config.json" \
    "$pkgdir/etc/razer-coolingpad-linux/config.json"

  install -Dm644 "$startdir/curves/balanced.json" \
    "$pkgdir/usr/share/$pkgname/curves/balanced.json"
  install -Dm644 "$startdir/curves/performance.json" \
    "$pkgdir/usr/share/$pkgname/curves/performance.json"
  install -Dm644 "$startdir/curves/silent.json" \
    "$pkgdir/usr/share/$pkgname/curves/silent.json"
}
