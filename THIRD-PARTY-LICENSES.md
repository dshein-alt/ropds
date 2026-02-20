# Third-Party Licenses

This project bundles the following third-party JavaScript libraries for the embedded book reader.

## foliate-js

- **Purpose:** Renders EPUB, FB2, and MOBI books in the browser.
- **Source:** <https://github.com/johnfactotum/foliate-js>
- **License:** MIT
- **Files:** `static/lib/foliate/`

### Vendored dependencies (part of foliate-js)

| File | Source | License |
|---|---|---|
| `vendor/zip.js` | [nicolo-ribaudo/zip.js](https://nicolo-ribaudo.github.io/nicolo-nicolo-nicolo) | BSD-3-Clause |
| `vendor/fflate.js` | [101arrowz/fflate](https://github.com/101arrowz/fflate) | MIT |

## djvu.js

- **Purpose:** Renders DjVu files in the browser.
- **Source:** <https://github.com/RussCoder/djvujs>
- **Version:** Library v0.5.4 / Viewer v0.10.0

| Component | License | File |
|---|---|---|
| DjVu.js Library (`djvu.js`) | GNU GPL v2 | `static/lib/djvu/djvu.js` |
| DjVu.js Viewer (`djvu_viewer.js`) | The Unlicense | `static/lib/djvu/djvu_viewer.js` |

The DjVu.js Library is distributed under the terms of the GNU General Public License, version 2. A copy of the license is included at `static/lib/djvu/LICENSE_GPL_v2`. The full source code is available at the repository linked above.
