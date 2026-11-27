pkgname=razer-coolingpad-linux
pkgver=0.3.0 # x-release-please-version
pkgrel=1
pkgdesc='Fan curve controller for the Razer Laptop Cooling Pad'
arch=('any')
url='https://github.com/matthewyjiang/razer-cooling-pad-linux'
license=('MIT')
depends=('python' 'python-hidapi' 'systemd')
install="$pkgname.install"
backup=('etc/razer-coolingpad-linux/config.json')
source=(
  '99-razer-coolingpad.rules'
  'LICENSE'
  'README.md'
  'config.json'
  'razer-coolingpad'
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
  install -Dm644 "$srcdir/99-razer-coolingpad.rules" \
    "$pkgdir/usr/lib/udev/rules.d/99-razer-coolingpad.rules"
  install -Dm644 "$srcdir/LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 "$srcdir/README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"

  install -Dm755 "$srcdir/razer-coolingpad" \
    "$pkgdir/usr/bin/razer-coolingpad"
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
