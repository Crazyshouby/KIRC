/* cdx-columns.js : colonnes redimensionnables des listes à en-têtes (un .lhead dont chaque enfant .lh est une colonne), pour toute la suite CDX.

   Usage :  CdxColumns.init({ head: document.getElementById("lhead"), cols: ["1fr", 170, 170, 90], key: "explorateur" });
   - cols : une largeur en pixels par colonne, ou "1fr" pour la colonne qui prend le reste de la place (une au plus).
   - Les lignes de la liste se mettent en grille sur la même variable, posée sur le parent de l'en-tête (la liste doit y être) :
         grid-template-columns: var(--cdx-cols, <largeurs d'origine>);
   - Une poignée se glisse sur le bord droit de chaque colonne sauf la dernière : le bord suit la souris. La largeur totale ne change pas : la place prise
     (ou rendue) l'est par la colonne « 1fr » si elle est à droite du bord, sinon par la colonne voisine de droite. Aucune colonne ne descend sous son
     minimum (48 px, et 80 px pour la colonne « 1fr »).
   - Les largeurs sont gardées par page (localStorage « cdx.cols.<key> ») ; un double-clic sur une poignée rend les largeurs d'origine.
   - Rayures : comme KDX, les colonnes d'une liste alternent deux fonds (jetons --col-a et --col-b) sur toute sa hauteur. La liste (le premier .list du
     parent, ou l'option `list`) reçoit --cdx-stripes, un dégradé par paliers aux largeurs réelles des en-têtes, recalculé quand elles changent. */
(() => {
  const MIN = 48, FLEX_MIN = 80;

  function init({ head, cols, key = "", min = MIN, scope, list }) {
    if (!head || !cols || !cols.length) return null;
    scope = scope || head.parentElement;
    list = list === undefined ? scope.querySelector(".list") : list;
    const kids = [...head.children].slice(0, cols.length), n = kids.length;
    if (n < 2) return null;
    const flex = cols.indexOf("1fr");                          // -1 : toutes les colonnes ont une largeur fixe
    const def = cols.map(c => (c === "1fr" ? 0 : c));
    let w = def.slice();
    try {
      const s = JSON.parse(localStorage.getItem("cdx.cols." + key));
      if (Array.isArray(s) && s.length === n && s.every((v, i) => i === flex || (typeof v === "number" && v >= min && v <= 3000))) w = s;
    } catch { /* pas de stockage, ou valeur illisible : largeurs d'origine */ }
    // Les rayures : un palier par colonne, à la largeur réelle de son en-tête (la colonne « 1fr » comprise), la dernière couleur jusqu'au bord.
    const stripes = () => {
      if (!list) return;
      let x = 0; const stops = [];
      kids.forEach((k, i) => { const wi = k.getBoundingClientRect().width; stops.push(`var(--col-${i % 2 ? "b" : "a"}) ${x}px ${i === n - 1 ? "100%" : (x + wi) + "px"}`); x += wi; });
      list.style.setProperty("--cdx-stripes", `linear-gradient(90deg, ${stops.join(", ")})`);
    };
    const apply = () => { scope.style.setProperty("--cdx-cols", w.map((v, i) => (i === flex ? `minmax(${FLEX_MIN}px, 1fr)` : v + "px")).join(" ")); requestAnimationFrame(stripes); };
    const save = () => { try { localStorage.setItem("cdx.cols." + key, JSON.stringify(w)); } catch { /* idem */ } };
    apply();
    if (list && window.ResizeObserver) new ResizeObserver(() => stripes()).observe(head);

    for (let i = 0; i < n - 1; i++) {
      const h = document.createElement("i"); h.className = "cdx-colrz"; h.setAttribute("aria-hidden", "true");
      kids[i].appendChild(h);
      let x0 = 0, a = 0, b = 0, drag = false;
      const j = flex > i ? flex : i + 1;                       // la colonne qui absorbe ce que la colonne i prend ou rend
      const lo = j => (j === flex ? FLEX_MIN : min);
      h.addEventListener("pointerdown", ev => {
        if (ev.button !== 0) return;
        ev.preventDefault(); ev.stopPropagation();
        drag = true; x0 = ev.clientX;
        a = kids[i].getBoundingClientRect().width; b = kids[j].getBoundingClientRect().width;   // largeurs réelles : celles de la colonne « 1fr » aussi
        try { h.setPointerCapture(ev.pointerId); } catch { /* idem */ }
        h.classList.add("drag"); document.body.classList.add("cdx-colsizing");
      });
      h.addEventListener("pointermove", ev => {
        if (!drag) return;
        const dx = Math.max(lo(i) - a, Math.min(b - lo(j), ev.clientX - x0));   // les deux colonnes gardent leur minimum
        if (i !== flex) w[i] = Math.round(a + dx);
        if (j !== flex) w[j] = Math.round(b - dx);
        apply();
      });
      const end = () => { if (!drag) return; drag = false; h.classList.remove("drag"); document.body.classList.remove("cdx-colsizing"); save(); };
      h.addEventListener("pointerup", end); h.addEventListener("pointercancel", end);
      h.addEventListener("click", ev => ev.stopPropagation());      // tirer une poignée ne doit pas trier la colonne
      h.addEventListener("dblclick", ev => { ev.stopPropagation(); w = def.slice(); apply(); save(); });
    }
    return { reset() { w = def.slice(); apply(); save(); }, widths: () => w.slice() };
  }

  window.CdxColumns = { init };
})();
