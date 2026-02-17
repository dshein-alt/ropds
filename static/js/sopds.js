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
