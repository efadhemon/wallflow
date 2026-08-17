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

  initFeatureCarousel();
  initHeroStage();

  function initHeroStage() {
    const stage = document.querySelector("[data-hero-stage]");
    if (!stage) return;

    const cards = Array.prototype.slice.call(stage.querySelectorAll(".hero-card"));
    const tabs = Array.prototype.slice.call(stage.querySelectorAll("[data-hero-tab]"));
    const caption = stage.querySelector("[data-hero-caption]");
    const captions = [
      "Search Wallhaven and add a wallpaper",
      "Folders you watch, files you add",
      "Countdown, pause, skip from the window",
    ];
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let index = 0;
    let timer = null;

    function show(next) {
      index = (next + cards.length) % cards.length;
      cards.forEach(function (card, i) {
        const pos = (i - index + cards.length) % cards.length;
        card.classList.toggle("is-front", pos === 0);
        card.classList.toggle("is-mid", pos === 1);
        card.classList.toggle("is-back", pos === 2);
      });
      tabs.forEach(function (tab, i) {
        tab.setAttribute("aria-selected", i === index ? "true" : "false");
      });
      if (caption) caption.textContent = captions[index];
    }

    function play() {
      if (reduced || timer) return;
      timer = window.setInterval(function () {
        show(index + 1);
      }, 4200);
    }

    function pause() {
      if (timer) {
        window.clearInterval(timer);
        timer = null;
      }
    }

    tabs.forEach(function (tab, i) {
      tab.addEventListener("click", function () {
        show(i);
        pause();
        play();
      });
    });

    stage.addEventListener("mouseenter", pause);
    stage.addEventListener("mouseleave", play);
    stage.addEventListener("focusin", pause);
    stage.addEventListener("focusout", function (event) {
      if (!stage.contains(event.relatedTarget)) play();
    });

    show(0);
    play();
  }

  function initFeatureCarousel() {
    const scroller = document.querySelector(".feature-track");
    const prev = document.querySelector('[data-carousel="prev"]');
    const next = document.querySelector('[data-carousel="next"]');
    const bar = document.querySelector(".carousel-progress-bar");
    if (!scroller || !prev || !next) return;

    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    function cardStep() {
      const card = scroller.querySelector("li");
      if (!card) return scroller.clientWidth * 0.8;
      const styles = window.getComputedStyle(scroller);
      const gap = parseFloat(styles.columnGap || styles.gap) || 16;
      return card.getBoundingClientRect().width + gap;
    }

    function update() {
      const max = Math.max(0, scroller.scrollWidth - scroller.clientWidth);
      prev.disabled = scroller.scrollLeft <= 4;
      next.disabled = scroller.scrollLeft >= max - 4;
      if (bar) {
        const seen = scroller.scrollLeft + scroller.clientWidth;
        const pct = scroller.scrollWidth <= 0 ? 100 : (seen / scroller.scrollWidth) * 100;
        bar.style.width = Math.min(100, Math.max(8, pct)) + "%";
      }
    }

    function move(direction) {
      scroller.scrollBy({
        left: direction * cardStep(),
        behavior: reduced ? "auto" : "smooth",
      });
    }

    prev.addEventListener("click", function () {
      move(-1);
    });
    next.addEventListener("click", function () {
      move(1);
    });
    scroller.addEventListener(
      "keydown",
      function (event) {
        if (event.key === "ArrowLeft") {
          event.preventDefault();
          move(-1);
        } else if (event.key === "ArrowRight") {
          event.preventDefault();
          move(1);
        }
      },
    );
    scroller.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    update();
  }

  function detectPlatform() {
    const uaData = navigator.userAgentData;
    const platformHint = ((uaData && uaData.platform) || navigator.platform || "").toLowerCase();
    const ua = (navigator.userAgent || "").toLowerCase();

    if (platformHint.includes("mac") || ua.includes("mac os") || ua.includes("macintosh")) {
      return "macos";
    }
    if (platformHint.includes("win") || ua.includes("windows")) {
      return "windows";
    }
    if (platformHint.includes("linux") || ua.includes("linux") || ua.includes("x11")) {
      return "linux";
    }
    return null;
  }
})();
