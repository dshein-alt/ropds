/**
 * reader.js — Embedded book reader controller.
 *
 * Reads config from data attributes on <body>, dispatches to the right
 * renderer (foliate-js for epub/fb2/mobi, djvu.js for djvu, native embed
 * for pdf), and handles position saving.
 */

const body = document.body;
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
    }).catch(() => {}); // fire-and-forget
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
    const embed = document.createElement('embed');
    embed.src = bookUrl;
    embed.type = 'application/pdf';
    embed.style.width = '100%';
    embed.style.height = '100%';
    embed.style.border = 'none';
    container.appendChild(embed);
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

    // Init viewer
    const viewer = new DjVu.Viewer();
    viewer.render(viewerDiv);

    // Fetch book data and load
    const resp = await fetch(bookUrl);
    const buf = await resp.arrayBuffer();
    viewer.loadDocument(buf);

    // Restore saved page position
    if (savedPos) {
        const page = parseInt(savedPos, 10);
        if (page > 0) {
            // DjVu.js uses 1-based page numbers
            setTimeout(() => viewer.setPageByNumber(page), 500);
        }
    }

    // Track page changes for position saving
    let lastPage = 0;
    setInterval(() => {
        try {
            const page = viewer.getPageNumber();
            if (page && page !== lastPage) {
                lastPage = page;
                currentPosition = String(page);
                const total = viewer.getPagesQuantity ? viewer.getPagesQuantity() : 0;
                if (total > 0) {
                    updateProgress(page / total);
                }
                debouncedSave();
            }
        } catch (_) {}
    }, 2000);
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

    // Register the custom element if not already
    if (!customElements.get('foliate-view')) {
        customElements.define('foliate-view', View);
    }

    const view = document.createElement('foliate-view');
    view.style.width = '100%';
    view.style.height = '100%';
    container.appendChild(view);

    // Open from URL
    await view.open(bookUrl);

    // Listen for position changes
    view.addEventListener('relocate', ({ detail }) => {
        if (detail.cfi) {
            currentPosition = detail.cfi;
        }
        if (typeof detail.fraction === 'number') {
            updateProgress(detail.fraction);
        }
        debouncedSave();
    });

    // Apply theme-aware styles
    const applyTheme = () => {
        const isDark = document.documentElement.getAttribute('data-bs-theme') === 'dark';
        view.renderer.setStyles?.(`
            html {
                color: ${isDark ? '#dee2e6' : '#212529'};
                background: ${isDark ? '#212529' : '#ffffff'};
            }
        `);
    };

    // Observe theme changes
    const observer = new MutationObserver(applyTheme);
    observer.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ['data-bs-theme'],
    });

    // Init: restore position or go to start
    if (savedPos) {
        await view.init({ lastLocation: savedPos });
    } else {
        view.renderer.next();
    }
    applyTheme();

    // Keyboard navigation
    document.addEventListener('keydown', (e) => {
        if (e.target !== document.body) return;
        if (e.key === 'ArrowLeft') view.goLeft();
        else if (e.key === 'ArrowRight') view.goRight();
    });
}

// ── History sidebar: load book in current reader ────────────────

window.loadBook = function(newBookId, newFormat) {
    // Navigate to the new book in the same reader tab
    window.location.href = '/web/reader/' + newBookId;
};
