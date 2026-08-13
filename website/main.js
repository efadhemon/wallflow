(function () {
  const nav = document.getElementById("site-nav");
  const toggle = document.querySelector(".nav-toggle");

  if (toggle && nav) {
    toggle.addEventListener("click", function () {
      const open = nav.classList.toggle("is-open");
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
    });

    nav.querySelectorAll("a").forEach(function (link) {
      link.addEventListener("click", function () {
        nav.classList.remove("is-open");
        toggle.setAttribute("aria-expanded", "false");
      });
    });
  }

  document.querySelectorAll('a[href^="#"]').forEach(function (anchor) {
    anchor.addEventListener("click", function (event) {
      const id = anchor.getAttribute("href");
      if (!id || id === "#") return;
      const target = document.querySelector(id);
      if (!target) return;
      event.preventDefault();
      target.scrollIntoView({ behavior: "smooth", block: "start" });
      history.replaceState(null, "", id);
    });
  });

  const platform = detectPlatform();
  const cards = document.querySelectorAll(".download-card[data-os]");

  cards.forEach(function (card) {
    const badge = document.createElement("p");
    badge.className = "recommended-badge";
    badge.textContent = "For your system";
    card.insertBefore(badge, card.querySelector("h3"));
  });

  if (platform) {
    cards.forEach(function (card) {
      if (card.getAttribute("data-os") === platform) {
        card.classList.add("recommended");
      }
    });
  }

  function detectPlatform() {
    const uaData = navigator.userAgentData;
    const platformHint = (
      (uaData && uaData.platform) ||
      navigator.platform ||
      ""
    ).toLowerCase();
    const ua = (navigator.userAgent || "").toLowerCase();

    if (
      platformHint.includes("mac") ||
      ua.includes("mac os") ||
      ua.includes("macintosh")
    ) {
      return "macos";
    }
    if (platformHint.includes("win") || ua.includes("windows")) {
      return "windows";
    }
    if (
      platformHint.includes("linux") ||
      ua.includes("linux") ||
      ua.includes("x11")
    ) {
      return "linux";
    }
    return null;
  }
})();
