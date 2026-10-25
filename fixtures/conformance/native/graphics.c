/* Original offscreen GLES probes: dynamic Android ABI, real pixels. */
#include <EGL/egl.h>
#include <GLES2/gl2.h>
#include <dlfcn.h>
#include <jni.h>
#include <stdio.h>
#include <string.h>
static jstring result(JNIEnv *e, const char *s) {
  return (*e)->NewStringUTF(e, s);
}
#define LOAD(type, name, library)          \
  type name = (type)dlsym(library, #name); \
  if (!name) return result(e, "UNSUPPORTED:" #name)
jstring as_probe_graphics(JNIEnv *e, int mode) {
  void *egl = dlopen("libEGL.so", RTLD_NOW),
       *gl = dlopen("libGLESv2.so", RTLD_NOW);
  if (!egl || !gl)
    return result(e, "UNSUPPORTED:EGL/GLES2 libraries unavailable");
  LOAD(PFNEGLGETDISPLAYPROC, eglGetDisplay, egl);
  LOAD(PFNEGLINITIALIZEPROC, eglInitialize, egl);
  LOAD(PFNEGLCHOOSECONFIGPROC, eglChooseConfig, egl);
  LOAD(PFNEGLCREATEPBUFFERSURFACEPROC, eglCreatePbufferSurface, egl);
  LOAD(PFNEGLCREATECONTEXTPROC, eglCreateContext, egl);
  LOAD(PFNEGLMAKECURRENTPROC, eglMakeCurrent, egl);
  LOAD(PFNEGLDESTROYSURFACEPROC, eglDestroySurface, egl);
  LOAD(PFNEGLDESTROYCONTEXTPROC, eglDestroyContext, egl);
  LOAD(PFNEGLGETCURRENTCONTEXTPROC, eglGetCurrentContext, egl);
  LOAD(PFNEGLGETCURRENTDISPLAYPROC, eglGetCurrentDisplay, egl);
  LOAD(PFNEGLGETCURRENTSURFACEPROC, eglGetCurrentSurface, egl);
  LOAD(PFNGLGETSTRINGPROC, glGetString, gl);
  LOAD(PFNGLGETERRORPROC, glGetError, gl);
  LOAD(PFNGLCLEARCOLORPROC, glClearColor, gl);
  LOAD(PFNGLCLEARPROC, glClear, gl);
  LOAD(PFNGLREADPIXELSPROC, glReadPixels, gl);
  LOAD(PFNGLCREATESHADERPROC, glCreateShader, gl);
  LOAD(PFNGLSHADERSOURCEPROC, glShaderSource, gl);
  LOAD(PFNGLCOMPILESHADERPROC, glCompileShader, gl);
  LOAD(PFNGLGETSHADERIVPROC, glGetShaderiv, gl);
  LOAD(PFNGLDELETESHADERPROC, glDeleteShader, gl);
  LOAD(PFNGLGENBUFFERSPROC, glGenBuffers, gl);
  LOAD(PFNGLBINDBUFFERPROC, glBindBuffer, gl);
  LOAD(PFNGLBUFFERDATAPROC, glBufferData, gl);
  LOAD(PFNGLGETBUFFERPARAMETERIVPROC, glGetBufferParameteriv, gl);
  LOAD(PFNGLDELETEBUFFERSPROC, glDeleteBuffers, gl);
  LOAD(PFNGLGENTEXTURESPROC, glGenTextures, gl);
  LOAD(PFNGLBINDTEXTUREPROC, glBindTexture, gl);
  LOAD(PFNGLTEXIMAGE2DPROC, glTexImage2D, gl);
  LOAD(PFNGLTEXPARAMETERIPROC, glTexParameteri, gl);
  LOAD(PFNGLDELETETEXTURESPROC, glDeleteTextures, gl);
  LOAD(PFNGLGENFRAMEBUFFERSPROC, glGenFramebuffers, gl);
  LOAD(PFNGLBINDFRAMEBUFFERPROC, glBindFramebuffer, gl);
  LOAD(PFNGLFRAMEBUFFERTEXTURE2DPROC, glFramebufferTexture2D, gl);
  LOAD(PFNGLCHECKFRAMEBUFFERSTATUSPROC, glCheckFramebufferStatus, gl);
  LOAD(PFNGLDELETEFRAMEBUFFERSPROC, glDeleteFramebuffers, gl);
  EGLDisplay previous_display = eglGetCurrentDisplay(),
             display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
  EGLContext previous_context = eglGetCurrentContext();
  EGLSurface previous_draw = eglGetCurrentSurface(EGL_DRAW),
             previous_read = eglGetCurrentSurface(EGL_READ);
  EGLint major = 0, minor = 0, count = 0;
  if (display == EGL_NO_DISPLAY || !eglInitialize(display, &major, &minor))
    return result(e, "UNSUPPORTED:EGL display initialization");
  const EGLint attrs[] = {EGL_SURFACE_TYPE,
                          EGL_PBUFFER_BIT,
                          EGL_RENDERABLE_TYPE,
                          EGL_OPENGL_ES2_BIT,
                          EGL_RED_SIZE,
                          8,
                          EGL_GREEN_SIZE,
                          8,
                          EGL_BLUE_SIZE,
                          8,
                          EGL_ALPHA_SIZE,
                          8,
                          EGL_NONE};
  const EGLint size[] = {EGL_WIDTH, 4, EGL_HEIGHT, 4, EGL_NONE},
               version[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
  EGLConfig config;
  if (!eglChooseConfig(display, attrs, &config, 1, &count) || count < 1)
    return result(e, "UNSUPPORTED:RGBA8 ES2 pbuffer configuration");
  EGLSurface surface = eglCreatePbufferSurface(display, config, size);
  EGLContext context =
      eglCreateContext(display, config, EGL_NO_CONTEXT, version);
  if (surface == EGL_NO_SURFACE || context == EGL_NO_CONTEXT) {
    if (surface != EGL_NO_SURFACE) eglDestroySurface(display, surface);
    if (context != EGL_NO_CONTEXT) eglDestroyContext(display, context);
    return result(e, "FAIL:EGL context/surface creation");
  }
  if (!eglMakeCurrent(display, surface, surface, context)) {
    eglDestroyContext(display, context);
    eglDestroySurface(display, surface);
    return result(e, "FAIL:eglMakeCurrent");
  }
  char out[2048];
  strcpy(out, "FAIL:unknown GLES probe");
  if (mode < 4) {
    const GLenum keys[] = {GL_VENDOR, GL_RENDERER, GL_VERSION, GL_EXTENSIONS};
    const char *s = (const char *)glGetString(keys[mode]);
    int length = snprintf(out, sizeof(out), "%s", s ? s : "FAIL:null GL string");
    if (length >= (int)sizeof(out)) {
      const char suffix[] = " [truncated]";
      memcpy(out + sizeof(out) - sizeof(suffix), suffix, sizeof(suffix));
    }
  }
  if (mode == 4) {
    unsigned char pixel[4] = {0};
    glClearColor(1, 0, 0, 1);
    glClear(GL_COLOR_BUFFER_BIT);
    glReadPixels(0, 0, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel);
    snprintf(out, sizeof(out), "%srgba=%u,%u,%u,%u",
             pixel[0] > 250 && pixel[1] < 5 && pixel[2] < 5 ? "" : "FAIL:",
             pixel[0], pixel[1], pixel[2], pixel[3]);
  }
  if (mode == 5 || mode == 6) {
    GLuint shader = glCreateShader(GL_VERTEX_SHADER);
    const char *source = mode == 5
                             ? "attribute vec4 p;void main(){gl_Position=p;}"
                             : "invalid shader intentionally";
    glShaderSource(shader, 1, &source, NULL);
    glCompileShader(shader);
    GLint ok = 0;
    glGetShaderiv(shader, GL_COMPILE_STATUS, &ok);
    glDeleteShader(shader);
    snprintf(out, sizeof(out), "%scompile=%d",
             (mode == 5 && ok) || (mode == 6 && !ok) ? "" : "FAIL:", ok);
  }
  if (mode == 7) {
    GLuint buffer = 0;
    GLint n = 0;
    float values[4] = {1, 2, 3, 4};
    glGenBuffers(1, &buffer);
    glBindBuffer(GL_ARRAY_BUFFER, buffer);
    glBufferData(GL_ARRAY_BUFFER, sizeof(values), values, GL_STATIC_DRAW);
    glGetBufferParameteriv(GL_ARRAY_BUFFER, GL_BUFFER_SIZE, &n);
    glDeleteBuffers(1, &buffer);
    snprintf(out, sizeof(out), "%sbufferBytes=%d",
             n == (int)sizeof(values) ? "" : "FAIL:", n);
  }
  if (mode == 8) {
    GLuint texture = 0, fbo = 0;
    unsigned char pixel[4] = {0};
    glGenTextures(1, &texture);
    glBindTexture(GL_TEXTURE_2D, texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, 4, 4, 0, GL_RGBA, GL_UNSIGNED_BYTE,
                 NULL);
    glGenFramebuffers(1, &fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D,
                           texture, 0);
    GLenum status = glCheckFramebufferStatus(GL_FRAMEBUFFER);
    if (status == GL_FRAMEBUFFER_COMPLETE) {
      glClearColor(0, 1, 0, 1);
      glClear(GL_COLOR_BUFFER_BIT);
      glReadPixels(0, 0, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel);
    }
    snprintf(out, sizeof(out), "%sfbo=%u green=%u",
             status == GL_FRAMEBUFFER_COMPLETE && pixel[1] > 250 ? "" : "FAIL:",
             status, pixel[1]);
    glBindFramebuffer(GL_FRAMEBUFFER, 0);
    glDeleteFramebuffers(1, &fbo);
    glDeleteTextures(1, &texture);
  }
  GLenum error = glGetError();
  if (error) snprintf(out, sizeof(out), "FAIL:GLES error 0x%x", error);
  /* Restore host state and destroy only resources created by this probe. The
   * default display may belong to the framework; never eglTerminate it. */
  EGLBoolean restored = previous_context != EGL_NO_CONTEXT
                            ? eglMakeCurrent(previous_display, previous_draw,
                                             previous_read, previous_context)
                            : eglMakeCurrent(display, EGL_NO_SURFACE,
                                             EGL_NO_SURFACE, EGL_NO_CONTEXT);
  if (!restored)
    return result(
        e, "FAIL:previous EGL context restoration; probe resources retained");
  if (!eglDestroyContext(display, context) ||
      !eglDestroySurface(display, surface))
    return result(e, "FAIL:EGL resource cleanup");
  return result(e, out);
}
