// Loaded before App and its dependencies. Vite lowers syntax, not built-in APIs.
import 'core-js/modules/es.array.at.js';
import 'core-js/modules/es.object.has-own.js';
import 'core-js/modules/es.string.replace-all.js';

// xterm subscribes to a media query as soon as its module is evaluated (before Vue mounts).
// Pre-Safari 14 WebKit supports the same change notifications through add/removeListener only.
if (typeof matchMedia === 'function') {
  const proto = Object.getPrototypeOf(matchMedia('all')) as MediaQueryList;
  if (typeof proto.addEventListener !== 'function' && typeof proto.addListener === 'function') {
    for (const [method, legacy] of [['addEventListener', 'addListener'], ['removeEventListener', 'removeListener']] as const) {
      Object.defineProperty(proto, method, { configurable: true, writable: true,
        value(this: MediaQueryList, type: string, listener: (event: MediaQueryListEvent) => void) {
          if (type === 'change') this[legacy](listener);
        },
      });
    }
  }
}
