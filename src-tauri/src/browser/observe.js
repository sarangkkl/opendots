(() => {
  if (!window.__openDots) {
    const state = {documentId: `${Date.now()}-${Math.random()}`, revision: 0, elements: [], roots: new WeakSet()};
    window.__openDots = state;
    state.watch = root => {
      if (state.roots.has(root)) return;
      state.roots.add(root);
      new MutationObserver(() => state.revision++).observe(root, {subtree:true, childList:true, attributes:true, characterData:true});
      root.addEventListener('input', () => state.revision++, true);
      root.addEventListener('scroll', () => state.revision++, true);
    };
  }
  const s = window.__openDots;
  s.elements = [];
  const limitations = [];
  const walk = (root, ox = 0, oy = 0) => {
    s.watch(root);
    for (const e of root.querySelectorAll('*')) {
      if (s.elements.length >= 500) break;
      const r = e.getBoundingClientRect();
      if (e.shadowRoot) walk(e.shadowRoot, ox, oy);
      if (e.tagName === 'IFRAME') {
        try { if (e.contentDocument) walk(e.contentDocument, ox+r.x+e.clientLeft, oy+r.y+e.clientTop); else limitations.push('Cross-origin frame requires its own tab; inaccessible frame omitted'); }
        catch { limitations.push('Cross-origin frame omitted'); }
      }
      if (r.width <= 0 || r.height <= 0 || e.getAttribute('aria-hidden') === 'true') continue;
      if (!e.matches('a,button,input,textarea,select,canvas,[role="button"],[role="link"],[contenteditable="true"],[tabindex]')) continue;
      const secret = e.matches('input[type=password]');
      const item = {index:s.elements.length, tag:e.tagName.toLowerCase(), role:e.getAttribute('role')||'', type:e.getAttribute('type')||'', name:(e.getAttribute('aria-label')||e.getAttribute('placeholder')||e.innerText||e.getAttribute('name')||'').slice(0,180), x:ox+r.x, y:oy+r.y, width:r.width, height:r.height, secret};
      s.elements.push({element:e, item});
    }
  };
  walk(document);
  return {url:location.href, title:document.title, documentId:s.documentId, revision:s.revision, viewport:{width:innerWidth,height:innerHeight,scrollX,scrollY,deviceScaleFactor:devicePixelRatio}, elements:s.elements.map(x=>x.item), limitations:[...new Set(limitations)]};
})()
