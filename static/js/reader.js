/**
 * reader.js — Embedded book reader controller.
 *
 * Reads config from data attributes on <body>, dispatches to the right
 * renderer (foliate-js for epub/fb2/mobi, djvu.js for djvu, native embed
 * for pdf), and handles position saving.
 */

const body = document.body;

// Fix mobile viewport height so header/footer stay visible on real devices.
function fixViewportHeight() {
    const viewportHeight = window.visualViewport?.height || window.innerHeight;
    body.style.height = Math.round(viewportHeight) + 'px';
}
fixViewportHeight();
window.addEventListener('resize', fixViewportHeight);
window.visualViewport?.addEventListener('resize', fixViewportHeight);
window.visualViewport?.addEventListener('scroll', fixViewportHeight);

const bookId    = parseInt(body.dataset.bookId, 10);
const format    = body.dataset.format;
const bookUrl   = body.dataset.bookUrl;
const savedPos  = body.dataset.savedPosition || '';
const savedProg = parseFloat(body.dataset.savedProgress) || 0;
const csrfToken = body.dataset.csrfToken || '';

const container = document.getElementById('reader-container');
const progressBadge = document.getElementById('reader-progress');

let currentPosition = savedPos;
let currentProgress = savedProg;
let saveTimer = null;

// ── Position Persistence ────────────────────────────────────────

function updateProgress(fraction) {
    currentProgress = fraction;
    if (progressBadge) {
        progressBadge.textContent = Math.round(fraction * 100) + '%';
    }
}

function savePosition() {
    if (!csrfToken || !currentPosition) return;
    const payload = JSON.stringify({
        book_id: bookId,
        position: currentPosition,
        progress: currentProgress,
        csrf_token: csrfToken,
    });
    fetch('/web/api/reading-position', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: payload,
    }).then(() => refreshHistorySidebar()).catch(() => {});
}

function refreshHistorySidebar() {
    const list = document.getElementById('history-list');
    if (!list) return;
    fetch('/web/api/reading-history')
        .then(r => r.json())
        .then(items => {
            list.innerHTML = items.map(item => {
                // For the current book use live JS progress instead of DB value
                const progress = item.book_id === bookId ? currentProgress : item.progress;
                const pct = Math.round(progress * 100);
                const active = item.book_id === bookId ? ' active' : '';
                return `<a href="/web/reader/${item.book_id}"
                    class="list-group-item list-group-item-action py-2 px-3${active}"
                    data-book-id="${item.book_id}"
                    onclick="event.preventDefault(); loadBook(${item.book_id}, '${item.format}');">
                    <div class="d-flex justify-content-between align-items-start">
                        <div class="text-truncate me-2 small">${item.title}</div>
                        <span class="badge bg-primary rounded-pill">${pct}%</span>
                    </div>
                    <small class="text-body-secondary">${item.updated_at}</small>
                </a>`;
            }).join('');
        })
        .catch(() => {});
}

// Refresh sidebar with live progress when the offcanvas is opened
const historyOffcanvas = document.getElementById('reading-history');
if (historyOffcanvas) {
    historyOffcanvas.addEventListener('show.bs.offcanvas', refreshHistorySidebar);
}

function savePositionBeacon() {
    if (!csrfToken || !currentPosition) return;
    const payload = JSON.stringify({
        book_id: bookId,
        position: currentPosition,
        progress: currentProgress,
        csrf_token: csrfToken,
    });
    navigator.sendBeacon(
        '/web/api/reading-position',
        new Blob([payload], { type: 'application/json' })
    );
}

function debouncedSave() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(savePosition, 10000);
}

// Save on tab close / switch
window.addEventListener('beforeunload', savePositionBeacon);
document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') savePositionBeacon();
});

// ── Auto-hide top toolbar on touch devices ──────────────────────

const header = document.getElementById('reader-header');
const isTouchDevice = matchMedia('(hover: none)').matches;

if (header && isTouchDevice) {
    let hideTimer = null;

    const showHeader = () => {
        header.classList.remove('collapsed');
        clearTimeout(hideTimer);
        hideTimer = setTimeout(() => header.classList.add('collapsed'), 3000);
    };

    const toggleHeader = () => {
        if (header.classList.contains('collapsed')) {
            showHeader();
        } else {
            clearTimeout(hideTimer);
            header.classList.add('collapsed');
        }
    };

    // Tap on reader area toggles toolbar
    container.addEventListener('click', (e) => {
        // Ignore clicks on nav zone buttons (they handle navigation)
        if (e.target.closest('.reader-nav-zone')) return;
        toggleHeader();
    });

    // Auto-hide after 3s on load
    hideTimer = setTimeout(() => header.classList.add('collapsed'), 3000);
}

// ── Format Dispatch ─────────────────────────────────────────────

if (format === 'pdf') {
    initPdfReader();
} else if (format === 'djvu') {
    initDjvuReader();
} else {
    initFoliateReader(); // epub, fb2, mobi
}

// ── PDF: native browser embed ───────────────────────────────────

function initPdfReader() {
    // Native embed doesn't expose page info to JS, so set a placeholder
    // position to ensure save handlers don't bail on empty currentPosition.
    if (!currentPosition) currentPosition = '1';
    const embed = document.createElement('embed');
    embed.src = bookUrl;
    embed.type = 'application/pdf';
    embed.style.width = '100%';
    embed.style.height = '100%';
    embed.style.border = 'none';
    container.appendChild(embed);
    savePosition();
}

// ── DJVU: djvu.js viewer ───────────────────────────────────────

async function initDjvuReader() {
    // Load djvu.js library on demand
    await loadScript('/static/lib/djvu/djvu.js');
    await loadScript('/static/lib/djvu/djvu_viewer.js');

    // Create viewer container
    const viewerDiv = document.createElement('div');
    viewerDiv.id = 'djvu-viewer';
    viewerDiv.style.width = '100%';
    viewerDiv.style.height = '100%';
    container.appendChild(viewerDiv);

    // Detect current theme
    const isDark = document.documentElement.getAttribute('data-bs-theme') === 'dark';

    // Init viewer with matching theme
    const viewer = new DjVu.Viewer();
    viewer.render(viewerDiv);
    viewer.configure({ theme: isDark ? 'dark' : 'light' });

    // Fetch book data and load (must await — returns a Promise)
    const resp = await fetch(bookUrl);
    const buf = await resp.arrayBuffer();
    await viewer.loadDocument(buf);

    // Restore saved page position via configure()
    if (savedPos) {
        const page = parseInt(savedPos, 10);
        if (page > 0) {
            viewer.configure({ pageNumber: page });
        }
    }

    // Track page changes via event (not polling)
    const Events = DjVu.Viewer.Events;
    const pagesSelector = DjVu.Viewer.get.pagesQuantity;

    viewer.on(Events.PAGE_NUMBER_CHANGED, () => {
        const page = viewer.getPageNumber();
        if (page) {
            currentPosition = String(page);
            try {
                const total = pagesSelector(viewer.store.getState());
                if (total > 0) updateProgress(page / total);
            } catch (_) {}
            debouncedSave();
        }
    });

    // Observe theme changes from the page toggle
    const observer = new MutationObserver(() => {
        const dark = document.documentElement.getAttribute('data-bs-theme') === 'dark';
        viewer.configure({ theme: dark ? 'dark' : 'light' });
    });
    observer.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ['data-bs-theme'],
    });
}

function loadScript(src) {
    return new Promise((resolve, reject) => {
        const s = document.createElement('script');
        s.src = src;
        s.onload = resolve;
        s.onerror = reject;
        document.head.appendChild(s);
    });
}

// ── Foliate: epub, fb2, mobi ───────────────────────────────────

async function initFoliateReader() {
    const { View } = await import('/static/lib/foliate/view.js');

    if (!customElements.get('foliate-view')) {
        customElements.define('foliate-view', View);
    }

    const view = document.createElement('foliate-view');
    view.style.width = '100%';
    view.style.height = '100%';
    container.appendChild(view);

    // Fetch book ourselves to provide a proper filename and MIME type to
    // foliate's makeBook() format detector.  The server URL /web/read/{id}
    // has no file extension, and foliate's fetchFile() would create a File
    // with no extension/type — causing FB2 detection to fail.
    const res = await fetch(bookUrl);
    if (!res.ok) throw new Error(`Failed to fetch book: ${res.status}`);
    const blob = await res.blob();
    const file = new File([blob], `book.${format}`, {
        type: res.headers.get('content-type')?.split(';')[0]?.trim() || '',
    });
    await view.open(file);

    // ── Footer toolbar elements ────────────────────────────────
    const locSpan   = document.getElementById('reader-loc');
    const slider    = document.getElementById('reader-slider');
    const btnPrev   = document.getElementById('btn-prev');
    const btnNext   = document.getElementById('btn-next');
    const zoomDown  = document.getElementById('zoom-down');
    const zoomUp    = document.getElementById('zoom-up');
    const gotoInput = document.getElementById('goto-input');
    const gotoBtn   = document.getElementById('goto-btn');

    let totalLocs = 1;

    // ── Background color presets ────────────────────────────────
    // Industry-standard reading colors from Kindle, Apple Books, Kobo
    const BG_PRESETS = [
        { id: 'auto',      label: 'Auto',      bg: null,      fg: null      },
        { id: 'white',     label: 'White',      bg: '#FFFFFF', fg: '#212529' },
        { id: 'sepia',     label: 'Sepia',      bg: '#FBF0D9', fg: '#5F4B32' },
        { id: 'cream',     label: 'Cream',      bg: '#F8F1E3', fg: '#3B3225' },
        { id: 'parchment', label: 'Parchment',  bg: '#EAE4D3', fg: '#433628' },
        { id: 'silver',    label: 'Silver',     bg: '#E0E0E0', fg: '#303030' },
        { id: 'dusk',      label: 'Dusk',       bg: '#3C3C3C', fg: '#C9CACA' },
        { id: 'night',     label: 'Night',      bg: '#121212', fg: '#B0B0B0' },
    ];

    let bgPreset = localStorage.getItem('reader-bg') || 'auto';
    const swatchContainer = document.getElementById('bg-swatches');

    // Build swatch rows (vertical list: dot + label)
    if (swatchContainer) {
        BG_PRESETS.forEach(p => {
            const row = document.createElement('div');
            row.className = 'bg-swatch-row' + (p.id === bgPreset ? ' active' : '');
            row.dataset.id = p.id;

            const dot = document.createElement('div');
            dot.className = 'swatch-dot';
            if (p.bg) {
                dot.style.background = p.bg;
            } else {
                dot.style.background = 'linear-gradient(135deg, #fff 50%, #212529 50%)';
            }

            const label = document.createElement('span');
            label.className = 'swatch-label';
            label.textContent = p.label;

            row.appendChild(dot);
            row.appendChild(label);

            row.addEventListener('click', (e) => {
                e.stopPropagation(); // keep dropdown open for easy switching
                bgPreset = p.id;
                localStorage.setItem('reader-bg', bgPreset);
                swatchContainer.querySelectorAll('.bg-swatch-row').forEach(s =>
                    s.classList.toggle('active', s.dataset.id === bgPreset));
                applyTheme();
            });
            swatchContainer.appendChild(row);
        });
    }

    // ── Zoom ───────────────────────────────────────────────────
    let zoomLevel = parseInt(localStorage.getItem('reader-zoom')) || 100;

    function applyZoom(delta) {
        if (delta) zoomLevel = Math.max(60, Math.min(200, zoomLevel + delta));
        localStorage.setItem('reader-zoom', zoomLevel);
        applyTheme();
    }

    zoomDown.addEventListener('click', () => applyZoom(-10));
    zoomUp.addEventListener('click', () => applyZoom(10));

    // ── Theme + zoom + bg color (combined in one setStyles call) ─
    const applyTheme = () => {
        const preset = BG_PRESETS.find(p => p.id === bgPreset);
        let fg, bg;
        if (preset?.bg) {
            // Explicit preset overrides theme
            fg = preset.fg;
            bg = preset.bg;
        } else {
            // Auto: follow page theme
            const isDark = document.documentElement.getAttribute('data-bs-theme') === 'dark';
            fg = isDark ? '#dee2e6' : '#212529';
            bg = isDark ? '#212529' : '#ffffff';
        }
        view.renderer.setStyles?.(`
            html {
                color: ${fg} !important;
                background: ${bg} !important;
                font-size: ${zoomLevel}% !important;
            }
            body, p, div, span, li, td, th, blockquote,
            h1, h2, h3, h4, h5, h6, a, em, strong, section, article {
                color: inherit !important;
                background: transparent !important;
            }
            a { text-decoration: underline; }
            img, svg, video, canvas { max-width: 100% !important; }
        `);
    };

    const themeObserver = new MutationObserver(applyTheme);
    themeObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ['data-bs-theme'],
    });

    // ── Navigation: keyboard in parent + iframe documents ──────
    const handleKeyNav = (e) => {
        if (e.key === 'ArrowLeft')  { e.preventDefault(); view.goLeft(); }
        else if (e.key === 'ArrowRight') { e.preventDefault(); view.goRight(); }
    };
    document.addEventListener('keydown', handleKeyNav);

    // ── Navigation: mouse wheel with cooldown ──────────────────
    let wheelLock = false;
    const handleWheel = (e) => {
        if (wheelLock) return;
        e.preventDefault();
        wheelLock = true;
        if (e.deltaY > 0 || e.deltaX > 0) view.goRight();
        else view.goLeft();
        setTimeout(() => { wheelLock = false; }, 300);
    };
    container.addEventListener('wheel', handleWheel, { passive: false });

    // Attach keyboard + wheel to each iframe doc — must be registered BEFORE
    // view.init() so we catch the very first section load.
    view.addEventListener('load', ({ detail: { doc } }) => {
        doc.addEventListener('keydown', handleKeyNav);
        doc.addEventListener('wheel', handleWheel, { passive: false });
    });

    // ── Progress slider state (declared early — used in relocate handler) ─
    let sliderDragging = false;

    // ── Position changes (page stats + progress) ───────────────
    view.addEventListener('relocate', ({ detail }) => {
        if (detail.cfi) currentPosition = detail.cfi;
        if (typeof detail.fraction === 'number') {
            updateProgress(detail.fraction);
            if (!sliderDragging) slider.value = Math.round(detail.fraction * 1000);
        }
        if (detail.location) {
            totalLocs = detail.location.total || 1;
            locSpan.textContent = `${detail.location.current} / ${totalLocs}`;
            gotoInput.max = totalLocs;
            gotoInput.placeholder = `1 – ${totalLocs}`;
        }
        debouncedSave();
    });

    // ── Init: restore position or go to start ──────────────────
    if (savedPos) {
        await view.init({ lastLocation: savedPos });
    } else {
        view.renderer.next();
    }
    applyTheme();

    // ── Navigation: buttons ────────────────────────────────────
    btnPrev.addEventListener('click', () => view.goLeft());
    btnNext.addEventListener('click', () => view.goRight());

    // ── Navigation: overlay zones on left/right edges ─────────
    // Always-visible semi-transparent arrows; brighter on hover.
    // Hidden on touch devices (@media hover:none) — swipe works natively.
    const leftZone = document.createElement('div');
    leftZone.className = 'reader-nav-zone reader-nav-zone--left';
    leftZone.innerHTML = '<div class="reader-nav-zone__icon"><i class="bi bi-chevron-left"></i></div>';
    leftZone.addEventListener('click', () => view.goLeft());

    const rightZone = document.createElement('div');
    rightZone.className = 'reader-nav-zone reader-nav-zone--right';
    rightZone.innerHTML = '<div class="reader-nav-zone__icon"><i class="bi bi-chevron-right"></i></div>';
    rightZone.addEventListener('click', () => view.goRight());

    container.appendChild(leftZone);
    container.appendChild(rightZone);

    // ── Progress slider ────────────────────────────────────────
    slider.addEventListener('input', () => { sliderDragging = true; });
    slider.addEventListener('change', () => {
        sliderDragging = false;
        const frac = parseInt(slider.value, 10) / 1000;
        view.goToFraction(Math.max(0, Math.min(1, frac)));
    });

    // ── GoTo dialog ────────────────────────────────────────────
    const doGoto = () => {
        const loc = parseInt(gotoInput.value, 10);
        if (!loc || loc < 1 || !totalLocs) return;
        const frac = Math.min(1, loc / totalLocs);
        view.goToFraction(frac);
        // Close the dropdown
        const toggle = document.getElementById('goto-toggle');
        bootstrap.Dropdown.getOrCreateInstance(toggle)?.hide();
    };
    gotoBtn.addEventListener('click', doGoto);
    gotoInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); doGoto(); }
        e.stopPropagation(); // prevent arrow keys from turning pages
    });
}

// ── History sidebar: load book in current reader ────────────────

window.loadBook = function(newBookId, newFormat) {
    // Navigate to the new book in the same reader tab
    window.location.href = '/web/reader/' + newBookId;
};
