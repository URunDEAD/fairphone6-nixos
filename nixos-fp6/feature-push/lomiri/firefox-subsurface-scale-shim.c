// fp6: LD_PRELOAD shim for Firefox on Mir 2.15 (Lomiri), which has no
// wp_viewporter. Firefox >= ~125 sizes its content wl_subsurface only through
// wp_viewport.set_destination; without a viewport the HiDPI buffer (1116 px
// wide) is shown as 1116 *logical* px, i.e. 3x too large. This shim gives
// such a subsurface its parent's wl_surface.set_buffer_scale before each
// commit, which is what firefox-wayland-buffer-scale-fallback.patch does in
// Firefox itself.
//
// It stays out of the way when:
//  * the process is not Firefox (LD_PRELOAD is inherited by helpers it runs),
//  * the subsurface has a wp_viewport (compositor with viewporter),
//  * the client already set a buffer scale on the subsurface itself.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client-core.h>
#include <wayland-util.h>

// libwayland's public wl_proxy starts with its wl_object.
struct fp6_obj { const struct wl_interface *iface; const void *impl; uint32_t id; };

#define FP6_MAXID 65536
#define FP6_MAXARGS 20
static int scale_of[FP6_MAXID];   // last set_buffer_scale by the client
static int applied[FP6_MAXID];    // scale currently pending/applied
static int own_scale[FP6_MAXID];  // client set a scale on this surface itself
static int viewported[FP6_MAXID]; // surface has a wp_viewport
static uint32_t parent_of[FP6_MAXID];
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static int enabled = -1;

typedef struct wl_proxy *(*arr_fn)(struct wl_proxy *, uint32_t,
                                   const struct wl_interface *, uint32_t,
                                   uint32_t, union wl_argument *);
static arr_fn real_arr;

static int is_enabled(void) {
  if (enabled < 0) {
    char exe[512];
    ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
    exe[n > 0 ? n : 0] = 0;
    const char *base = strrchr(exe, '/');
    base = base ? base + 1 : exe;
    enabled = strncmp(base, "firefox", 7) == 0;
  }
  return enabled;
}

static const char *next_arg(const char *s, char *type) {
  for (; *s; s++) {
    if ((*s >= '0' && *s <= '9') || *s == '?') continue;
    *type = *s;
    return s + 1;
  }
  *type = 0;
  return s;
}

static uint32_t obj_id(void *o) {
  return o ? wl_proxy_get_id((struct wl_proxy *)o) : 0;
}

struct wl_proxy *wl_proxy_marshal_flags(struct wl_proxy *p, uint32_t opcode,
                                        const struct wl_interface *iface,
                                        uint32_t version, uint32_t flags, ...) {
  if (!real_arr)
    real_arr = (arr_fn)dlsym(RTLD_NEXT, "wl_proxy_marshal_array_flags");
  const struct wl_interface *pi = ((struct fp6_obj *)p)->iface;
  union wl_argument a[FP6_MAXARGS];
  memset(a, 0, sizeof a);
  va_list ap;
  va_start(ap, flags);
  char t;
  int n = 0;
  for (const char *s = next_arg(pi->methods[opcode].signature, &t);
       t && n < FP6_MAXARGS; s = next_arg(s, &t), n++) {
    switch (t) {
      case 'i': a[n].i = va_arg(ap, int32_t); break;
      case 'u': a[n].u = va_arg(ap, uint32_t); break;
      case 'f': a[n].f = va_arg(ap, wl_fixed_t); break;
      case 's': a[n].s = va_arg(ap, const char *); break;
      case 'o': a[n].o = va_arg(ap, struct wl_object *); break;
      case 'n': (void)va_arg(ap, void *); a[n].o = NULL; break;
      case 'a': a[n].a = va_arg(ap, struct wl_array *); break;
      case 'h': a[n].h = va_arg(ap, int32_t); break;
    }
  }
  va_end(ap);

  if (is_enabled()) {
    uint32_t id = wl_proxy_get_id(p);
    if (!strcmp(pi->name, "wl_surface") && id < FP6_MAXID) {
      int inject = 0;
      pthread_mutex_lock(&mu);
      if (opcode == 0) {  // destroy: the id may be reused
        scale_of[id] = applied[id] = own_scale[id] = viewported[id] = 0;
        parent_of[id] = 0;
      } else if (opcode == 8) {  // set_buffer_scale
        scale_of[id] = applied[id] = a[0].i;
        own_scale[id] = 1;
      } else if (opcode == 6) {  // commit
        uint32_t par = parent_of[id];
        int want = par < FP6_MAXID ? scale_of[par] : 0;
        if (par && want > 1 && !own_scale[id] && !viewported[id] &&
            applied[id] != want) {
          applied[id] = want;
          inject = want;
        }
      }
      pthread_mutex_unlock(&mu);
      if (inject) {
        union wl_argument s[1];
        s[0].i = inject;
        real_arr(p, 8, NULL, wl_proxy_get_version(p), 0, s);
      }
    } else if (!strcmp(pi->name, "wl_subcompositor") && opcode == 1) {
      uint32_t c = obj_id(a[1].o), par = obj_id(a[2].o);  // get_subsurface
      if (c < FP6_MAXID) {
        pthread_mutex_lock(&mu);
        parent_of[c] = par;
        applied[c] = 0;
        pthread_mutex_unlock(&mu);
      }
    } else if (!strcmp(pi->name, "wp_viewporter") && opcode == 1) {
      uint32_t s = obj_id(a[1].o);  // get_viewport(new_id, surface)
      if (s < FP6_MAXID) {
        pthread_mutex_lock(&mu);
        viewported[s] = 1;
        pthread_mutex_unlock(&mu);
      }
    }
  }
  return real_arr(p, opcode, iface, version, flags, a);
}
