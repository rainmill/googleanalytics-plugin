// The Google Analytics tracker (rainmill.googleanalytics). Configured by the
// data-lat-* attributes on its own <script> tag; the site decides about consent, and
// this script only offers the switch.
//
//   latAnalytics.grant()            visit statistics allowed ({ ads: true } also allows ad signals)
//   latAnalytics.deny()             refused; analytics cookies removed
//   latAnalytics.state()            "granted", "denied" or "unset"
//   latAnalytics.event(name, data)  queued until the tag runs, dropped on deny
//   lat:analytics                   a document event on every change, and once when unset
//
// Attributes on any element: data-analytics-grant, data-analytics-deny,
// data-analytics-ask (asks again), and data-track="event" with data-track-<param>.
(function () {
  var me = document.currentScript;
  if (!me || window.latAnalytics) return;
  var cfg = me.dataset;
  var id = cfg.latMeasurementId || "";
  var tag = cfg.latTagId || "";
  var consent = cfg.latConsent || "none";
  var remember = (cfg.latRemember || "").toLowerCase() === "true";
  var regions = (cfg.latRegions || "").split(",").map(function (r) { return r.trim(); }).filter(Boolean);
  var KEY = cfg.latStorageKey || "lat-analytics";

  window.dataLayer = window.dataLayer || [];
  function gtag() { window.dataLayer.push(arguments); }

  function stored() {
    if (!remember) return null;
    try { return localStorage.getItem(KEY); } catch (e) { return null; }
  }
  function keep(value) {
    if (!remember) return;
    try { localStorage.setItem(KEY, value); } catch (e) { /* asked again on the next page */ }
  }

  // "yes" and "no" are read as an answer too, as cookie notices commonly store them.
  var saved = ({ yes: "granted", no: "denied" })[stored()] || stored() || "";
  var adsAllowed = saved === "granted:ads";
  var state = consent === "none" ? "granted" : saved.indexOf("granted") === 0 ? "granted" : saved === "denied" ? "denied" : "unset";
  var loaded = false;
  var queue = [];

  function signals(granted, ads) {
    var v = granted ? "granted" : "denied";
    var a = granted && ads ? "granted" : "denied";
    return { analytics_storage: v, ad_storage: a, ad_user_data: a, ad_personalization: a };
  }

  function load() {
    if (loaded || (!id && !tag)) return;
    loaded = true;
    if (id) window["ga-disable-" + id] = false;
    gtag("js", new Date());
    if (id) {
      var options = {};
      if (cfg.latCookieDomain) options.cookie_domain = cfg.latCookieDomain;
      gtag("config", id, options);
    }
    var s = document.createElement("script");
    s.async = true;
    s.src = tag
      ? "https://www.googletagmanager.com/gtm.js?id=" + encodeURIComponent(tag)
      : "https://www.googletagmanager.com/gtag/js?id=" + encodeURIComponent(id);
    document.head.appendChild(s);
    queue.splice(0).forEach(function (e) { gtag("event", e[0], e[1]); });
  }

  function forget() {
    if (id) window["ga-disable-" + id] = true;
    document.cookie.split(";").forEach(function (pair) {
      var name = pair.split("=")[0].trim();
      if (name.indexOf("_ga") !== 0) return;
      var gone = name + "=; Max-Age=0; path=/";
      document.cookie = gone;
      document.cookie = gone + "; domain=." + location.hostname;
      if (cfg.latCookieDomain) document.cookie = gone + "; domain=" + cfg.latCookieDomain;
    });
  }

  function announce(asked) {
    document.dispatchEvent(new CustomEvent("lat:analytics", { detail: { state: state, ask: !!asked } }));
  }

  if (consent === "mode") {
    var defaults = signals(false, false);
    defaults.wait_for_update = 500;
    if (regions.length) defaults.region = regions;
    gtag("consent", "default", defaults);
    if (state === "granted") gtag("consent", "update", signals(true, adsAllowed));
    load();
  } else if (state === "granted") {
    load();
  } else if (state === "denied") {
    forget();
  }

  var api = {
    grant: function (options) {
      var ads = !!(options && options.ads);
      state = "granted";
      keep(ads ? "granted:ads" : "granted");
      if (consent === "mode") gtag("consent", "update", signals(true, ads));
      else load();
      announce(false);
    },
    deny: function () {
      var was = state;
      state = "denied";
      keep("denied");
      queue.length = 0;
      if (consent === "mode") gtag("consent", "update", signals(false, false));
      if (was === "granted") forget();
      announce(false);
      // Tag Manager alone has no switch to stop a running tag; the page reloads once.
      if (was === "granted" && loaded && !id && consent !== "mode") location.reload();
    },
    state: function () { return state; },
    event: function (name, data) {
      if (!name || state === "denied" && consent !== "mode") return;
      if (loaded) gtag("event", name, data || {});
      else queue.push([name, data || {}]);
    },
    ask: function () { announce(true); },
  };
  window.latAnalytics = api;

  document.addEventListener("click", function (e) {
    var el = e.target.closest && e.target.closest("[data-analytics-grant], [data-analytics-deny], [data-analytics-ask], [data-track]");
    if (!el) return;
    if (el.hasAttribute("data-analytics-grant")) api.grant({ ads: el.getAttribute("data-analytics-grant") === "ads" });
    else if (el.hasAttribute("data-analytics-deny")) api.deny();
    else if (el.hasAttribute("data-analytics-ask")) api.ask();
    else {
      var data = {};
      Array.prototype.forEach.call(el.attributes, function (a) {
        if (a.name.indexOf("data-track-") === 0) data[a.name.slice(11).replace(/-/g, "_")] = a.value;
      });
      api.event(el.getAttribute("data-track"), data);
    }
  });

  if (state === "unset") {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", function () { announce(false); });
    else announce(false);
  }
})();
