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
