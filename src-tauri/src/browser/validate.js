(index => {
  const s=window.__openDots;
  if (!s) return false;
  const stored=s.elements[index];
  if (!stored?.element?.isConnected) return false;
  const e=stored.element;
  if(e.type==='password')return false;
  const r=e.getBoundingClientRect();
  let x=r.x,y=r.y,view=e.ownerDocument.defaultView;
  while(view!==window){const frame=view.frameElement;if(!frame)return false;const box=frame.getBoundingClientRect();x+=box.x+frame.clientLeft;y+=box.y+frame.clientTop;view=frame.ownerDocument.defaultView;}
  const old=stored.item;
  const root=e.getRootNode();
  const hit=root.elementFromPoint?.(r.x+r.width/2,r.y+r.height/2);
  if(hit && hit!==e && !e.contains(hit))return false;
  return Math.abs(x-old.x)<1 && Math.abs(y-old.y)<1 && Math.abs(r.width-old.width)<1 && Math.abs(r.height-old.height)<1;
})
