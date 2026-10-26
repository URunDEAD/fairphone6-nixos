/* Android API33 AArch64 GLES2 first-draw fixture. Execute only in an explicitly
 * prepared fresh Android trial. Uses Android SYSTEM EGL/GLES libraries and
 * normal ro.hardware.egl/gralloc selection, never host GLVND or direct vendor
 * dlopen. No window/screen, network, audio or camera APIs. Default rejects all
 * software renderers; --allow-software is a labelled VM control only.
 */
#define _GNU_SOURCE
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <ctype.h>
#include <errno.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

enum { WIDTH = 64, HEIGHT = 64 };

static void watchdog(int signal_number)
{
    (void)signal_number;
    static const char message[] = "FAIL watchdog: 12-second userspace deadline; no A810 pass\n";
    ssize_t written = write(STDERR_FILENO, message, sizeof message - 1);
    (void)written;
    _exit(124);
}

static bool gl_ok(const char *where)
{
    GLenum error;
    bool ok = true;
    for (unsigned i = 0; i < 8; ++i) {
        error = glGetError();
        if (error == GL_NO_ERROR)
            return ok;
        fprintf(stderr, "FAIL GL %s error=0x%04x\n", where, error);
        ok = false;
    }
    fprintf(stderr, "FAIL GL %s error-drain limit reached\n", where);
    return false;
}

static GLuint shader(GLenum type, const char *source)
{
    GLuint object = glCreateShader(type);
    GLint compiled = GL_FALSE;
    if (!object) {
        fprintf(stderr, "FAIL glCreateShader type=0x%x\n", type);
        return 0;
    }
    glShaderSource(object, 1, &source, NULL);
    glCompileShader(object);
    glGetShaderiv(object, GL_COMPILE_STATUS, &compiled);
    if (!compiled || !gl_ok("shader compile")) {
        char log[4096];
        GLsizei length = 0;
        glGetShaderInfoLog(object, sizeof log, &length, log);
        fprintf(stderr, "FAIL shader type=0x%x compile=%d log=%.*s\n",
                type, compiled, (int)length, log);
        glDeleteShader(object);
        return 0;
    }
    return object;
}

static bool contains_810(const char *text)
{
    for (const char *p = text; (p = strstr(p, "810")); ++p)
        if ((p == text || !isdigit((unsigned char)p[-1])) &&
            !isdigit((unsigned char)p[3]))
            return true;
    return false;
}

static bool software_renderer(const char *text)
{
    return strcasestr(text, "llvmpipe") || strcasestr(text, "softpipe") ||
           strcasestr(text, "swrast") || strcasestr(text, "swiftshader") ||
           strcasestr(text, "lavapipe") || strcasestr(text, "software");
}

static bool a810_renderer(const char *text)
{
    return !software_renderer(text) && contains_810(text) &&
           (strcasestr(text, "fd810") || strcasestr(text, "adreno"));
}

int main(int argc, char **argv)
{
    bool allow_software = false;
    int result = 1;
    EGLDisplay display = EGL_NO_DISPLAY;
    EGLContext context = EGL_NO_CONTEXT;
    bool current = false, initialized = false;
    GLuint vertex = 0, fragment = 0, program = 0, buffer = 0, texture = 0, framebuffer = 0;
    EGLint major_version = 0, minor_version = 0, count = 0;
    EGLConfig config = NULL;
    setvbuf(stdout, NULL, _IOLBF, 0);
    setvbuf(stderr, NULL, _IOLBF, 0);
    for (int i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--allow-software")) {
            allow_software = true;
        } else if (!strcmp(argv[i], "--help") || !strcmp(argv[i], "--identity-only")) {
            puts("Android API33 AArch64 GLES2 shader/FBO/readback4096 fixture");
            puts("Usage: fp6-android-gles-first-draw [--allow-software]");
            puts("Default requires real FD810/Adreno810; software control has NO hardware claim.");
            puts("Identity/help exits without calling any EGL/GL entry point.");
            return 0;
        } else {
            fprintf(stderr, "FAIL unknown option: %s\n", argv[i]);
            return 2;
        }
    }
    struct sigaction action = { .sa_handler = watchdog };
    sigemptyset(&action.sa_mask);
    if (sigaction(SIGALRM, &action, NULL) < 0) {
        perror("FAIL sigaction");
        return 2;
    }
    alarm(12);
    printf("MODE ANDROID_SYSTEM_EGL allow_software=%d size=%dx%d\n", allow_software, WIDTH, HEIGHT);
    puts("PHASE eglGetDisplay Android default begin");
    display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    result = 4;
    if (display == EGL_NO_DISPLAY) {
        fprintf(stderr, "FAIL eglGetDisplay error=0x%x\n", eglGetError());
        goto cleanup;
    }
    puts("PHASE eglInitialize begin");
    if (!eglInitialize(display, &major_version, &minor_version)) {
        fprintf(stderr, "FAIL eglInitialize error=0x%x\n", eglGetError());
        goto cleanup;
    }
    initialized = true;
    printf("EGL version=%d.%d vendor=%s\n", major_version, minor_version, eglQueryString(display, EGL_VENDOR));
    const char *extensions = eglQueryString(display, EGL_EXTENSIONS);
    if ((major_version < 1 || (major_version == 1 && minor_version < 5)) &&
        (!extensions || !strstr(extensions, "EGL_KHR_surfaceless_context"))) {
        fprintf(stderr, "FAIL EGL has no surfaceless context support\n");
        goto cleanup;
    }
    const EGLint config_attributes[] = {
        EGL_SURFACE_TYPE, 0, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8, EGL_NONE
    };
    const EGLint context_attributes[] = { EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE };
    if (!eglBindAPI(EGL_OPENGL_ES_API) ||
        !eglChooseConfig(display, config_attributes, &config, 1, &count) || count != 1) {
        fprintf(stderr, "FAIL EGL API/config count=%d error=0x%x\n", count, eglGetError());
        goto cleanup;
    }
    puts("PHASE eglCreateContext GLES2 begin");
    context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attributes);
    if (context == EGL_NO_CONTEXT) {
        fprintf(stderr, "FAIL eglCreateContext error=0x%x\n", eglGetError());
        goto cleanup;
    }
    puts("PHASE eglMakeCurrent surfaceless begin");
    if (!eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, context)) {
        fprintf(stderr, "FAIL eglMakeCurrent error=0x%x\n", eglGetError());
        goto cleanup;
    }
    current = true;
    const char *renderer = (const char *)glGetString(GL_RENDERER);
    const char *vendor = (const char *)glGetString(GL_VENDOR);
    const char *version = (const char *)glGetString(GL_VERSION);
    if (!renderer || !vendor || !version || !gl_ok("renderer strings")) {
        fprintf(stderr, "FAIL missing GL identity\n");
        goto cleanup;
    }
    printf("GL renderer=%s\nGL vendor=%s\nGL version=%s\n", renderer, vendor, version);
    if (!a810_renderer(renderer) && !(allow_software && software_renderer(renderer))) {
        fprintf(stderr, "FAIL renderer policy: expected FD810/Adreno810; software requires explicit control flag\n");
        result = 5;
        goto cleanup;
    }
    result = 6;
    puts("PHASE compile actual GLES2 vertex+fragment shaders begin");
    vertex = shader(GL_VERTEX_SHADER, "attribute vec2 position; void main(){gl_Position=vec4(position,0.0,1.0);}");
    fragment = shader(GL_FRAGMENT_SHADER, "precision mediump float; void main(){gl_FragColor=vec4(0.25,0.5,0.75,1.0);}");
    if (!vertex || !fragment)
        goto cleanup;
    program = glCreateProgram();
    glAttachShader(program, vertex);
    glAttachShader(program, fragment);
    glBindAttribLocation(program, 0, "position");
    glLinkProgram(program);
    GLint linked = GL_FALSE;
    glGetProgramiv(program, GL_LINK_STATUS, &linked);
    if (!linked || !gl_ok("program link")) {
        char log[4096];
        GLsizei length = 0;
        glGetProgramInfoLog(program, sizeof log, &length, log);
        fprintf(stderr, "FAIL program link=%d log=%.*s\n", linked, (int)length, log);
        goto cleanup;
    }
    result = 7;
    puts("PHASE create 64x64 RGBA texture FBO and VBO begin");
    glGenTextures(1, &texture);
    glBindTexture(GL_TEXTURE_2D, texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, WIDTH, HEIGHT, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    glGenFramebuffers(1, &framebuffer);
    glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0);
    GLenum complete = glCheckFramebufferStatus(GL_FRAMEBUFFER);
    GLint bits[4];
    const GLenum channels[] = { GL_RED_BITS, GL_GREEN_BITS, GL_BLUE_BITS, GL_ALPHA_BITS };
    for (int i = 0; i < 4; ++i)
        glGetIntegerv(channels[i], &bits[i]);
    printf("FBO status=0x%x RGBA_bits=%d/%d/%d/%d\n", complete, bits[0], bits[1], bits[2], bits[3]);
    if (complete != GL_FRAMEBUFFER_COMPLETE || bits[0] < 8 || bits[1] < 8 || bits[2] < 8 || bits[3] < 8 || !gl_ok("FBO"))
        goto cleanup;
    const GLfloat triangle[] = { -1.0f, -1.0f, 3.0f, -1.0f, -1.0f, 3.0f };
    glGenBuffers(1, &buffer);
    glBindBuffer(GL_ARRAY_BUFFER, buffer);
    glBufferData(GL_ARRAY_BUFFER, sizeof triangle, triangle, GL_STATIC_DRAW);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, NULL);
    glEnableVertexAttribArray(0);
    glUseProgram(program);
    glViewport(0, 0, WIDTH, HEIGHT);
    glDisable(GL_DITHER);
    glClearColor(0, 0, 0, 0);
    glClear(GL_COLOR_BUFFER_BIT);
    if (!gl_ok("draw setup"))
        goto cleanup;
    puts("PHASE glDrawArrays full-cover triangle begin");
    glDrawArrays(GL_TRIANGLES, 0, 3);
    if (!gl_ok("glDrawArrays"))
        goto cleanup;
    puts("PHASE glFinish begin");
    glFinish();
    if (!gl_ok("glFinish"))
        goto cleanup;
    puts("PHASE glReadPixels all4096 begin");
    unsigned char pixels[WIDTH * HEIGHT * 4];
    glPixelStorei(GL_PACK_ALIGNMENT, 1);
    glReadPixels(0, 0, WIDTH, HEIGHT, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
    if (!gl_ok("glReadPixels"))
        goto cleanup;
    const unsigned char expected[] = { 64, 128, 191, 255 };
    unsigned mismatches = 0;
    for (int pixel = 0; pixel < WIDTH * HEIGHT; ++pixel) {
        bool bad = false;
        for (int channel = 0; channel < 4; ++channel)
            bad |= abs((int)pixels[4 * pixel + channel] - expected[channel]) > 1;
        if (bad && mismatches++ < 3)
            fprintf(stderr, "FAIL pixel=%d RGBA=%u/%u/%u/%u\n", pixel,
                    pixels[4 * pixel], pixels[4 * pixel + 1], pixels[4 * pixel + 2], pixels[4 * pixel + 3]);
    }
    printf("READBACK pixels=%d mismatches=%u expected=64/128/191/255 tolerance=1 first=%u/%u/%u/%u\n",
           WIDTH * HEIGHT, mismatches, pixels[0], pixels[1], pixels[2], pixels[3]);
    if (mismatches)
        goto cleanup;
    result = 0;

cleanup:
    puts("PHASE cleanup begin");
    if (current) {
        glDeleteBuffers(1, &buffer);
        glDeleteFramebuffers(1, &framebuffer);
        glDeleteTextures(1, &texture);
        if (program) glDeleteProgram(program);
        if (vertex) glDeleteShader(vertex);
        if (fragment) glDeleteShader(fragment);
        if (!gl_ok("cleanup") && !result) result = 7;
        if (!eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT) && !result) result = 4;
    }
    if (context != EGL_NO_CONTEXT && !eglDestroyContext(display, context) && !result) result = 4;
    if (initialized && !eglTerminate(display) && !result) result = 4;
    if (!eglReleaseThread() && !result) result = 4;
    alarm(0);
    if (!result)
        printf("PASS %s pixels=4096 cleanup=complete A810_first_draw_claim=%s\n",
               allow_software ? "ANDROID_SOFTWARE_CONTROL" : "ANDROID_A810_FIRST_DRAW",
               allow_software ? "NO" : "YES");
    else
        fprintf(stderr, "FAIL exit=%d no A810 pass\n", result);
    return result;
}
