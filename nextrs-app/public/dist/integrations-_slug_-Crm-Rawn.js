import{C as e,Q as t,S as n,b as r,c as i,f as a,g as o,i as s,k as c,r as l,x as u,y as d}from"./chunks/src-D2ZlBWRH.js";
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const f=o(`Banknote`,[[`rect`,{width:`20`,height:`12`,x:`2`,y:`6`,rx:`2`,key:`9lu3g6`}],[`circle`,{cx:`12`,cy:`12`,r:`2`,key:`1c9p78`}],[`path`,{d:`M6 12h.01M18 12h.01`,key:`113zkx`}]]),p=o(`Percent`,[[`line`,{x1:`19`,x2:`5`,y1:`5`,y2:`19`,key:`1x9vlm`}],[`circle`,{cx:`6.5`,cy:`6.5`,r:`2.5`,key:`4mh3h7`}],[`circle`,{cx:`17.5`,cy:`17.5`,r:`2.5`,key:`1mdrzq`}]]),m=o(`TriangleAlert`,[[`path`,{d:`m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3`,key:`wmoenq`}],[`path`,{d:`M12 9v4`,key:`juzpu7`}],[`path`,{d:`M12 17h.01`,key:`p32p05`}]]),h=o(`WalletCards`,[[`rect`,{width:`18`,height:`18`,x:`3`,y:`3`,rx:`2`,key:`afitv7`}],[`path`,{d:`M3 9a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2`,key:`4125el`}],[`path`,{d:`M3 11h3c.8 0 1.6.3 2.1.9l1.1.9c1.6 1.6 4.1 1.6 5.7 0l1.1-.9c.5-.5 1.3-.9 2.1-.9H21`,key:`1dpki6`}]])
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
;var g=t();function _(){return(0,g.jsx)(l,{children:t=>{let o=t.overviews[0],l=t.loans.reduce((e,t)=>e+(t.principal_balance??0),0);return(0,g.jsxs)(i,{title:t.connection.name,description:`Portfolio position, current income, and the latest imported activity.`,children:[(0,g.jsx)(s,{data:t}),t.connection.last_error?(0,g.jsxs)(`div`,{className:`flex gap-3 rounded-xl border border-amber-300 bg-amber-50 p-4 text-sm text-amber-950`,children:[(0,g.jsx)(m,{className:`mt-0.5 size-4 shrink-0`}),t.connection.last_error]}):null,(0,g.jsxs)(`section`,{className:`grid gap-4 sm:grid-cols-2 xl:grid-cols-4`,children:[(0,g.jsx)(v,{label:`Portfolio value`,value:c(o?.portfolio_value??l),icon:(0,g.jsx)(a,{className:`size-4`})}),(0,g.jsx)(v,{label:`Trust balance`,value:c(o?.trust_balance),icon:(0,g.jsx)(h,{className:`size-4`})}),(0,g.jsx)(v,{label:`Portfolio yield`,value:o?.portfolio_yield==null?`—`:`${o.portfolio_yield.toFixed(2)}%`,icon:(0,g.jsx)(p,{className:`size-4`})}),(0,g.jsx)(v,{label:`YTD interest`,value:c(o?.ytd_interest),icon:(0,g.jsx)(f,{className:`size-4`})})]}),(0,g.jsxs)(`section`,{className:`grid gap-4 lg:grid-cols-2`,children:[(0,g.jsxs)(d,{children:[(0,g.jsxs)(n,{children:[(0,g.jsx)(e,{children:`Active loans`}),(0,g.jsxs)(u,{children:[t.loans.length,` loans currently imported.`]})]}),(0,g.jsx)(r,{className:`space-y-3`,children:t.loans.slice(0,6).map(e=>(0,g.jsxs)(`a`,{href:`/integrations/${t.connection.slug}/loans/${encodeURIComponent(e.loan_account)}`,className:`flex items-center justify-between rounded-lg border p-3 hover:bg-muted`,children:[(0,g.jsxs)(`div`,{children:[(0,g.jsx)(`p`,{className:`text-sm font-medium`,children:e.borrower_name||e.loan_account}),(0,g.jsx)(`p`,{className:`text-xs text-muted-foreground`,children:e.property_address||e.loan_account})]}),(0,g.jsx)(`span`,{className:`text-sm font-medium`,children:c(e.principal_balance)})]},e.loan_account))})]}),(0,g.jsxs)(d,{children:[(0,g.jsxs)(n,{children:[(0,g.jsx)(e,{children:`Recent payments`}),(0,g.jsx)(u,{children:`The latest imported provider activity.`})]}),(0,g.jsx)(r,{className:`space-y-3`,children:t.payments.slice(0,6).map(e=>(0,g.jsxs)(`div`,{className:`flex items-center justify-between border-b pb-3 last:border-0`,children:[(0,g.jsxs)(`div`,{children:[(0,g.jsx)(`p`,{className:`text-sm font-medium`,children:e.borrower_name||e.loan_account}),(0,g.jsxs)(`p`,{className:`text-xs text-muted-foreground`,children:[e.check_date,` · `,e.loan_account]})]}),(0,g.jsx)(`span`,{className:`text-sm font-medium text-primary`,children:c(e.amount)})]},e.id))})]})]})]})}})}function v({label:e,value:t,icon:n}){return(0,g.jsx)(d,{children:(0,g.jsxs)(r,{className:`pt-5`,children:[(0,g.jsxs)(`div`,{className:`flex items-center justify-between text-muted-foreground`,children:[(0,g.jsx)(`span`,{className:`text-xs font-medium uppercase tracking-wide`,children:e}),n]}),(0,g.jsx)(`p`,{className:`mt-3 text-2xl font-semibold`,children:t})]})})}export{_ as default};