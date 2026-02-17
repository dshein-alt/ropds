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
(function () {
  document.addEventListener("DOMContentLoaded", function () {
    document.querySelectorAll("time.utc-time").forEach(function (el) {
      var dt = new Date(el.getAttribute("datetime").replace(" ", "T"));
      if (isNaN(dt)) return;
      var pad = function (n) { return n < 10 ? "0" + n : n; };
      el.textContent =
        dt.getFullYear() + "-" + pad(dt.getMonth() + 1) + "-" + pad(dt.getDate()) +
        " " + pad(dt.getHours()) + ":" + pad(dt.getMinutes()) + ":" + pad(dt.getSeconds());
    });
  });
})();
