/* Loader-only probe: deliberately calls no EGL/GL/Vulkan/GBM entry point. */
#include <dlfcn.h>
#include <stdio.h>
int main(int argc,char **argv) {
  int failures=0;
  for(int i=1;i<argc;i++) {
    void *h=dlopen(argv[i],RTLD_NOW|RTLD_LOCAL);
    if(!h) {printf("FAIL %s: %s\n",argv[i],dlerror());failures++;}
    else {printf("LOADED %s\n",argv[i]);}
  }
  printf("loader-only result: %d failed; no graphics API called\n",failures);
  return failures?1:0;
}
