// Bundle the real page/query provider; only identity context and HTTP are fixtures.
import { build } from "esbuild";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
const output = resolve(".local/member-activity-ui");
mkdirSync(output, { recursive: true });
await build({
  stdin: {
    contents: `
      import React from 'react';
      import {createRoot} from 'react-dom/client';
      import {focusManager} from '@tanstack/react-query';
      import {Providers} from './apps/web/src/components/providers';
      import Home from './apps/web/src/app/page';
      window.setFocused = value => focusManager.setFocused(value);
      createRoot(document.getElementById('root')).render(<Providers><Home/></Providers>);
    `,
    resolveDir: process.cwd(),
    loader: "tsx",
  },
  bundle: true,
  outfile: resolve(output, "app.js"),
  jsx: "automatic",
  tsconfig: "apps/web/tsconfig.json",
  define: { "process.env.NODE_ENV": '"development"' },
  plugins: [
    {
      name: "fixtures",
      setup(builder) {
        builder.onResolve(
          { filter: /(?:app-context|features\/shell|^\.\/shell)$/ },
          (args) => ({ path: args.path, namespace: "fixture" }),
        );
        builder.onLoad({ filter: /.*/, namespace: "fixture" }, (args) => ({
          contents: args.path.includes("app-context")
            ? `export const useApp = () => ({ user: window.fixture.user, viewing: null }); export const useAction = () => ({run: async () => true, busy: false});`
            : `import {MembersPage} from './apps/web/src/features/members'; export const Workspace = MembersPage; export const Logo = () => null;`,
          resolveDir: process.cwd(),
          loader: "tsx",
        }));
      },
    },
  ],
});
writeFileSync(
  resolve(output, "index.html"),
  `<!doctype html><meta charset="utf-8"><div id="root"></div><script>
const base = Date.now();
window.fixture = {
  now: base, activity: new Date(base-4*3600000).toISOString(), calls: {}, failMe: false,
  user: {id:'fixture', name:'Activity fixture', email:'activity@example.invalid', is_admin:true, sync_approved:true}
};
Date.now = () => fixture.now;
const interval = window.setInterval.bind(window);
window.setInterval = (fn, ms, ...args) => interval(fn, ms === 15000 ? 150 : ms === 30000 ? 300 : ms, ...args);
window.fetch = async url => {
  const path = String(url).replace('/api/v1', '');
  fixture.calls[path] = (fixture.calls[path] || 0) + 1;
  if(path === '/auth/me' && fixture.unauthorized) return new Response('{}', {status:401});
  if(path === '/auth/me' && fixture.failMe) return new Response('{}', {status:503});
  const data = path === '/auth/me' ? fixture.user : path === '/admin/users' ? [{...fixture.user, created_at:new Date(base).toISOString(), ...(fixture.legacy ? {last_login:fixture.activity} : {last_active_at:fixture.activity})}] : path === '/health' ? {status:'ok'} : [];
  return new Response(JSON.stringify(data), {headers:{'Content-Type':'application/json'}});
};
</script><script src="app.js"></script>`,
);
