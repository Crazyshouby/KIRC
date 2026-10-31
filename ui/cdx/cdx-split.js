/* cdx-split.js : volets redimensionnables (la barre verticale entre deux zones côte à côte), pour toute la suite CDX.

   Usage :  CdxSplit.init({ pane: document.getElementById("side"), side: "left", key: "explorateur-dossiers", min: 150, max: 520 });
   - pane : le volet à largeur fixe, enfant d'un conteneur flex en rangée ; l'autre zone (celle qui prend le reste, flex-grow > 0) s'adapte.
   - side : "left" si le volet est à gauche de la zone souple (la barre est sur son bord droit), "right" s'il est à sa droite (barre sur son bord gauche).
   - min, max : bornes de la largeur du volet en pixels ; minOther : largeur minimale de la zone souple (200 par défaut).
   La barre est posée dans l'espace (gap) du conteneur : elle ne change pas la mise en page. Elle suit le volet quand il est caché (attribut hidden, display: none).
   La largeur est gardée par page (localStorage « cdx.split.<key> ») ; un double-clic sur la barre rend la largeur d'origine (celle du CSS). */
(() => {
  function init({ pane, side = "left", key = "", min = 120, max = 0, minOther = 200 }) {
    const parent = pane && pane.parentElement;
    if (!parent) return null;
    const gap = parseFloat(getComputedStyle(parent).columnGap) || 0;
    const storeKey = "cdx.split." + key;
    const flexEl = () => [...parent.children].find(c => c !== pane && !c.classList.contains("cdx-split") && parseFloat(getComputedStyle(c).flexGrow) > 0);

    // Les bornes remplacent celles du CSS du volet (min-width / max-width), qui servaient à une largeur en pourcentage.
    pane.style.flex = "none"; pane.style.minWidth = min + "px"; pane.style.maxWidth = max ? max + "px" : "none";
    const css0 = pane.style.width;                              // largeur posée en ligne par la page, s'il y en a une (sinon celle du CSS)
    const setW = w => { pane.style.width = Math.round(w) + "px"; };
    const reset = () => { pane.style.width = css0; };
    try { const s = parseFloat(localStorage.getItem(storeKey)); if (s >= min && (!max || s <= max)) setW(s); } catch { /* pas de stockage */ }

    const h = document.createElement("div"); h.className = "cdx-split"; h.setAttribute("aria-hidden", "true");
    h.style.marginLeft = -gap + "px";
    const strip = document.createElement("i"); const sw = Math.max(gap + 2, 8);
    strip.style.width = sw + "px"; strip.style.left = -((sw - gap) / 2) + "px";
    h.appendChild(strip);
    parent.insertBefore(h, side === "left" ? pane.nextSibling : pane);

    // La barre disparaît avec son volet.
    const sync = () => { h.hidden = pane.hidden || getComputedStyle(pane).display === "none"; };
    sync(); new MutationObserver(sync).observe(pane, { attributes: true, attributeFilter: ["hidden", "style", "class"] });

    let x0 = 0, w0 = 0, f0 = 0, drag = false;
    strip.addEventListener("pointerdown", ev => {
      if (ev.button !== 0) return;
      ev.preventDefault(); ev.stopPropagation();
      const f = flexEl(); if (!f) return;
      drag = true; x0 = ev.clientX; w0 = pane.getBoundingClientRect().width; f0 = f.getBoundingClientRect().width;
      try { strip.setPointerCapture(ev.pointerId); } catch { /* idem */ }
      h.classList.add("drag"); document.body.classList.add("cdx-colsizing");
    });
    strip.addEventListener("pointermove", ev => {
      if (!drag) return;
      const dx = (ev.clientX - x0) * (side === "left" ? 1 : -1);
      const hi = Math.min(max || Infinity, w0 + f0 - minOther);   // la zone souple garde sa largeur minimale
      setW(Math.max(min, Math.min(Math.max(min, hi), w0 + dx)));
    });
    const end = () => { if (!drag) return; drag = false; want = null; h.classList.remove("drag"); document.body.classList.remove("cdx-colsizing"); try { localStorage.setItem(storeKey, String(Math.round(pane.getBoundingClientRect().width))); } catch { /* idem */ } };
    strip.addEventListener("pointerup", end); strip.addEventListener("pointercancel", end);
    strip.addEventListener("click", ev => ev.stopPropagation());
    strip.addEventListener("dblclick", ev => { ev.stopPropagation(); reset(); try { localStorage.removeItem(storeKey); } catch { /* idem */ } });

    // Une fenêtre qui rétrécit ne doit pas écraser la zone souple : le volet cède de la place, puis la reprend quand la fenêtre s'agrandit (sans toucher à la largeur gardée).
    let want = null;
    const fit = () => {
      const f = flexEl(); if (!f || h.hidden || drag) return;
      const w = pane.getBoundingClientRect().width, avail = w + f.getBoundingClientRect().width - minOther;
      if (avail < w - 0.5) { if (want === null) want = w; setW(Math.max(min, avail)); }
      else if (want !== null && w < want - 0.5) { setW(Math.min(want, avail)); if (Math.min(want, avail) >= want) want = null; }
    };
    if (window.ResizeObserver) new ResizeObserver(fit).observe(parent);
    return { reset, fit, width: () => pane.getBoundingClientRect().width };
  }

  window.CdxSplit = { init };
})();
