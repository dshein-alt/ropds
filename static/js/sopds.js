// Theme toggle (persists in localStorage)
(function () {
  const THEME_KEY = "sopds-theme";

  function getPreferred() {
    const saved = localStorage.getItem(THEME_KEY);
    if (saved) return saved;
    return document.documentElement.getAttribute("data-bs-theme") || "light";
  }

  function apply(theme) {
    document.documentElement.setAttribute("data-bs-theme", theme);
    localStorage.setItem(THEME_KEY, theme);
    // Update toggle button icon
    const icon = document.getElementById("theme-icon");
    if (icon) {
      icon.className = theme === "dark" ? "bi bi-sun" : "bi bi-moon";
    }
  }

  // Apply saved theme immediately
  apply(getPreferred());

  // Expose toggle function
  window.toggleTheme = function () {
    const current = document.documentElement.getAttribute("data-bs-theme");
    apply(current === "dark" ? "light" : "dark");
  };
})();

// Search type switcher
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    const form = document.getElementById("search-form");
    const radios = document.querySelectorAll('input[name="search-target"]');
    if (!form || !radios.length) return;
    // Keep form action in sync with the currently selected radio on initial load.
    const selected = document.querySelector('input[name="search-target"]:checked');
    if (selected && selected.dataset.action) {
      form.action = selected.dataset.action;
    }
    radios.forEach(function (radio) {
      radio.addEventListener("change", function () {
        form.action = this.dataset.action;
      });
    });
  });
})();

// Language selector: redirect to current page instead of /web
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    var links = document.querySelectorAll("a.lang-link");
    var currentPath = window.location.pathname;
    links.forEach(function (link) {
      var url = new URL(link.href, window.location.origin);
      url.searchParams.set("redirect", currentPath);
      link.href = url.toString();
    });
  });
})();

// Toggle password visibility
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    document.querySelectorAll(".toggle-password").forEach(function (btn) {
      btn.addEventListener("click", function () {
        var targetId = this.getAttribute("data-target");
        var input = document.getElementById(targetId);
        if (!input) return;
        var isPassword = input.type === "password";
        input.type = isPassword ? "text" : "password";
        var icon = this.querySelector("i");
        if (icon) {
          icon.className = isPassword ? "bi bi-eye-slash" : "bi bi-eye";
        }
      });
    });
  });
})();

// Password confirmation validation
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    document.querySelectorAll("input[data-confirm-for]").forEach(function (confirmInput) {
      var form = confirmInput.closest("form");
      if (!form) return;
      form.addEventListener("submit", function (e) {
        var targetId = confirmInput.getAttribute("data-confirm-for");
        var passwordInput = document.getElementById(targetId);
        if (!passwordInput) return;
        if (passwordInput.value !== confirmInput.value) {
          e.preventDefault();
          confirmInput.classList.add("is-invalid");
        } else {
          confirmInput.classList.remove("is-invalid");
        }
      });
      confirmInput.addEventListener("input", function () {
        this.classList.remove("is-invalid");
      });
    });
  });
})();

// Admin: populate shared change-password modal
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    document.querySelectorAll(".btn-pw-change").forEach(function (btn) {
      btn.addEventListener("click", function () {
        var userId = this.getAttribute("data-user-id");
        var username = this.getAttribute("data-username");
        var form = document.getElementById("pwModalForm");
        var title = document.getElementById("pwModalTitle");
        if (form) form.action = "/web/admin/users/" + userId + "/password";
        if (title) title.textContent = username;
        var modal = new bootstrap.Modal(document.getElementById("pwModal"));
        modal.show();
      });
    });
    var pwModal = document.getElementById("pwModal");
    if (pwModal) {
      pwModal.addEventListener("hidden.bs.modal", function () {
        var form = this.querySelector("form");
        if (form) form.reset();
        this.querySelectorAll(".is-invalid").forEach(function (el) { el.classList.remove("is-invalid"); });
        this.querySelectorAll(".toggle-password i").forEach(function (el) { el.className = "bi bi-eye"; });
        this.querySelectorAll("input[type='text'][data-target]").forEach(function () {});
        var inputs = this.querySelectorAll("input[name='password'], input[data-confirm-for]");
        inputs.forEach(function (el) { el.type = "password"; });
      });
    }
  });
})();

// Admin: populate shared delete-confirmation modal
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    document.querySelectorAll(".btn-del-user").forEach(function (btn) {
      btn.addEventListener("click", function () {
        var userId = this.getAttribute("data-user-id");
        var username = this.getAttribute("data-username");
        var form = document.getElementById("delModalForm");
        var name = document.getElementById("delModalUsername");
        if (form) form.action = "/web/admin/users/" + userId + "/delete";
        if (name) name.textContent = username;
        var modal = new bootstrap.Modal(document.getElementById("delModal"));
        modal.show();
      });
    });
  });
})();

// Delete confirmation: require typing "delete"
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    document.querySelectorAll(".delete-confirm-input").forEach(function (input) {
      var modal = input.closest(".modal");
      if (!modal) return;
      var btn = modal.querySelector(".delete-confirm-btn");
      if (!btn) return;
      input.addEventListener("input", function () {
        btn.disabled = this.value.trim().toLowerCase() !== "delete";
      });
      modal.addEventListener("hidden.bs.modal", function () {
        input.value = "";
        btn.disabled = true;
      });
    });
  });
})();

// Flash messages from URL query params
// Pages provide window._flashMessages = { msg_key: "text" } and window._flashErrors = { err_key: "text" }
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    var params = new URLSearchParams(window.location.search);
    var flash = document.getElementById("flash-msg");
    var text = document.getElementById("flash-text");
    if (!flash || !text) return;

    var messages = window._flashMessages || {};
    var errors = window._flashErrors || {};
    var msg = params.get("msg");
    var err = params.get("error");

    if (msg && messages[msg]) {
      flash.classList.remove("d-none", "alert-danger");
      flash.classList.add("alert-success");
      text.textContent = messages[msg];
    } else if (err && errors[err]) {
      flash.classList.remove("d-none", "alert-success");
      flash.classList.add("alert-danger");
      text.textContent = errors[err];
    }

    if (msg || err) {
      window.history.replaceState({}, "", window.location.pathname);
    }
  });
})();

// Cover preview overlay
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    var overlay = document.getElementById("cover-overlay");
    var overlayImg = document.getElementById("cover-overlay-img");
    if (!overlay || !overlayImg) return;

    document.addEventListener("click", function (e) {
      var thumb = e.target.closest(".cover-preview");
      if (thumb && thumb.dataset.coverUrl) {
        e.preventDefault();
        overlayImg.src = thumb.dataset.coverUrl;
        overlay.hidden = false;
      }
    });

    overlay.addEventListener("click", function () {
      overlay.hidden = true;
      overlayImg.src = "";
    });

    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape" && !overlay.hidden) {
        overlay.hidden = true;
        overlayImg.src = "";
      }
    });
  });
})();

// Convert UTC timestamps to local timezone
function convertUtcTimes(root) {
  (root || document).querySelectorAll("time.utc-time").forEach(function (el) {
    var dt = new Date(el.getAttribute("datetime").replace(" ", "T"));
    if (isNaN(dt)) return;
    var pad = function (n) { return n < 10 ? "0" + n : n; };
    el.textContent =
      dt.getFullYear() + "-" + pad(dt.getMonth() + 1) + "-" + pad(dt.getDate()) +
      " " + pad(dt.getHours()) + ":" + pad(dt.getMinutes()) + ":" + pad(dt.getSeconds());
  });
}
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    convertUtcTimes(document);
  });
})();

// Bookshelf star toggle via AJAX (no page reload)
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    document.addEventListener("click", function (e) {
      var btn = e.target.closest(".bookshelf-toggle-btn");
      if (!btn) return;
      e.preventDefault();

      var form = btn.closest("form");
      if (!form) return;

      var body = new URLSearchParams(new FormData(form)).toString();
      btn.disabled = true;

      fetch(form.action, {
        method: "POST",
        headers: {
          "Content-Type": "application/x-www-form-urlencoded",
          "X-Requested-With": "XMLHttpRequest"
        },
        body: body,
        credentials: "same-origin"
      })
        .then(function (res) { return res.json(); })
        .then(function (data) {
          if (!data.ok) return;

          // On the bookshelf page, remove the card
          var isBookshelfPage = !!document.getElementById("bookshelf-grid");
          if (isBookshelfPage && !data.on_shelf) {
            var card = btn.closest(".col");
            if (card) {
              card.style.transition = "opacity 0.3s";
              card.style.opacity = "0";
              setTimeout(function () { card.remove(); }, 300);
            }
            return;
          }

          // On other pages, toggle the star appearance
          var icon = btn.querySelector("i");
          if (data.on_shelf) {
            btn.classList.remove("btn-outline-secondary");
            btn.classList.add("btn-warning");
            if (icon) { icon.classList.remove("bi-star"); icon.classList.add("bi-star-fill"); }
          } else {
            btn.classList.remove("btn-warning");
            btn.classList.add("btn-outline-secondary");
            if (icon) { icon.classList.remove("bi-star-fill"); icon.classList.add("bi-star"); }
          }
        })
        .finally(function () {
          btn.disabled = false;
        });
    });
  });
})();

// Bookshelf infinite scroll
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    var grid = document.getElementById("bookshelf-grid");
    var sentinel = document.getElementById("bookshelf-sentinel");
    var loader = document.getElementById("bookshelf-loader");
    if (!grid || !sentinel) return;

    var loading = false;
    var hasMore = grid.dataset.hasMore === "true";
    var offset = parseInt(grid.dataset.offset, 10) || 0;
    var sort = grid.dataset.sort || "date";
    var dir = grid.dataset.dir || "desc";

    function loadMore() {
      if (loading || !hasMore) return;
      loading = true;
      if (loader) loader.classList.remove("d-none");

      var url = "/web/bookshelf/cards?offset=" + offset + "&sort=" + sort + "&dir=" + dir;
      fetch(url, { credentials: "same-origin" })
        .then(function (res) { return res.json(); })
        .then(function (data) {
          if (data.html) {
            var tmp = document.createElement("div");
            tmp.innerHTML = data.html;
            while (tmp.firstElementChild) {
              grid.appendChild(tmp.firstElementChild);
            }
            convertUtcTimes(grid);
            offset += grid.querySelectorAll(".col").length - (offset);
            // Recount: offset = total loaded cards
            offset = grid.children.length;
          }
          hasMore = data.has_more;
          loading = false;
          if (loader) loader.classList.add("d-none");
        })
        .catch(function () {
          loading = false;
          if (loader) loader.classList.add("d-none");
        });
    }

    if ("IntersectionObserver" in window) {
      var observer = new IntersectionObserver(function (entries) {
        if (entries[0].isIntersecting && hasMore) {
          loadMore();
        }
      }, { rootMargin: "200px" });
      observer.observe(sentinel);
    }
  });
})();

// Genre selector utility (shared by upload page and book detail editor)
window.GenreSelector = (function () {
  var cachedSections = null;

  function fetchGenres() {
    if (cachedSections) return Promise.resolve(cachedSections);
    return fetch("/web/api/genres", { credentials: "same-origin" })
      .then(function (r) { return r.json(); })
      .then(function (data) {
        cachedSections = data.sections;
        return cachedSections;
      });
  }

  // Build accordion with checkboxes inside `container`.
  // `selectedIds` is an array of genre IDs to pre-check.
  // `selectedCodes` is an array of genre codes to pre-check (for upload flow).
  // `onChangeCallback(selectedIds)` is called on every toggle.
  function build(container, sections, opts) {
    opts = opts || {};
    var selectedIdSet = new Set((opts.selectedIds || []).map(Number));
    var selectedCodeSet = new Set(opts.selectedCodes || []);
    var onChange = opts.onChange || function () {};

    container.innerHTML = "";
    var index = 0;

    Object.keys(sections).forEach(function (sectionName) {
      var genres = sections[sectionName];
      var sectionId = "gsec-" + (index++);
      var count = genres.filter(function (g) {
        return selectedIdSet.has(g.id) || selectedCodeSet.has(g.code);
      }).length;

      var item = document.createElement("div");
      item.className = "accordion-item";
      item.innerHTML =
        '<h2 class="accordion-header">' +
        '<button class="accordion-button collapsed py-2 px-3 small" type="button" ' +
        'data-bs-toggle="collapse" data-bs-target="#' + sectionId + '">' +
        escHtml(sectionName) +
        ' <span class="badge bg-primary ms-2 gsec-count">' + count + '</span>' +
        '</button></h2>' +
        '<div id="' + sectionId + '" class="accordion-collapse collapse">' +
        '<div class="accordion-body py-2 px-3">' +
        genres.map(function (g) {
          var checked = (selectedIdSet.has(g.id) || selectedCodeSet.has(g.code)) ? " checked" : "";
          return '<div class="form-check">' +
            '<input class="form-check-input genre-cb" type="checkbox" value="' + g.id + '" data-code="' + g.code + '" id="gcb-' + g.id + '"' + checked + '>' +
            '<label class="form-check-label small" for="gcb-' + g.id + '">' + escHtml(g.subsection) + '</label>' +
            '</div>';
        }).join("") +
        '</div></div>';
      container.appendChild(item);
    });

    container.addEventListener("change", function (e) {
      if (!e.target.classList.contains("genre-cb")) return;
      var body = e.target.closest(".accordion-body");
      var header = body.parentElement.previousElementSibling;
      var badge = header.querySelector(".gsec-count");
      badge.textContent = body.querySelectorAll(".genre-cb:checked").length;
      onChange(getSelected(container));
    });
  }

  function getSelected(container) {
    var ids = [];
    container.querySelectorAll(".genre-cb:checked").forEach(function (cb) {
      ids.push(parseInt(cb.value, 10));
    });
    return ids;
  }

  function getCodes(container) {
    var codes = [];
    container.querySelectorAll(".genre-cb:checked").forEach(function (cb) {
      codes.push(cb.dataset.code);
    });
    return codes;
  }

  function escHtml(s) {
    var d = document.createElement("div");
    d.textContent = s;
    return d.innerHTML;
  }

  return {
    fetchGenres: fetchGenres,
    build: build,
    getSelected: getSelected,
    getCodes: getCodes
  };
})();
