// SPDX-License-Identifier: GPL-3.0-only
#include "screenrecorder.h"

#include <QDBusConnection>
#include <QDBusError>
#include <QDBusMessage>
#include <QDebug>
#include <QDir>
#include <QOpenGLContext>
#include <QOpenGLFunctions>
#include <QQuickWindow>
#include <QSettings>
#include <QStandardPaths>

#include <cstdio>
#include <cstring>
#include <ctime>
#include <errno.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <unistd.h>

#ifndef FP6_SCREENRECORD_BIN
#define FP6_SCREENRECORD_BIN "fp6-screenrecord"
#endif
#ifndef FP6_RECORD_WIDTH
#define FP6_RECORD_WIDTH 720
#endif
#ifndef FP6_RECORD_FPS
#define FP6_RECORD_FPS 30
#endif

// GLES 3.0 enums (Qt may be built against the GLES 2 / desktop headers).
#ifndef GL_READ_FRAMEBUFFER
#define GL_READ_FRAMEBUFFER 0x8CA8
#define GL_DRAW_FRAMEBUFFER 0x8CA9
#define GL_READ_FRAMEBUFFER_BINDING 0x8CAA
#define GL_DRAW_FRAMEBUFFER_BINDING 0x8CA6
#endif
#ifndef GL_PIXEL_PACK_BUFFER
#define GL_PIXEL_PACK_BUFFER 0x88EB
#define GL_PIXEL_PACK_BUFFER_BINDING 0x88ED
#endif
#ifndef GL_STREAM_READ
#define GL_STREAM_READ 0x88E1
#endif
#ifndef GL_MAP_READ_BIT
#define GL_MAP_READ_BIT 0x0001
#endif
#ifndef GL_SYNC_GPU_COMMANDS_COMPLETE
#define GL_SYNC_GPU_COMMANDS_COMPLETE 0x9117
#define GL_ALREADY_SIGNALED 0x911A
#define GL_CONDITION_SATISFIED 0x911C
#define GL_SYNC_FLUSH_COMMANDS_BIT 0x00000001
#endif
#ifndef GL_RGBA8
#define GL_RGBA8 0x8058
#endif

namespace {

// GLES 3.0 entry points, resolved from the context. Qt's own format for
// qtmir's context says "ES 2" whatever the driver gives, so the version is
// checked with glGetString and the functions are looked up by name.
struct Gl3 {
    QOpenGLFunctions *f = nullptr;
    void (*BlitFramebuffer)(GLint, GLint, GLint, GLint, GLint, GLint, GLint, GLint, GLbitfield, GLenum) = nullptr;
    void *(*FenceSync)(GLenum, GLbitfield) = nullptr;
    GLenum (*ClientWaitSync)(void *, GLbitfield, quint64) = nullptr;
    void (*DeleteSync)(void *) = nullptr;
    void *(*MapBufferRange)(GLenum, GLintptr, GLsizeiptr, GLbitfield) = nullptr;
    GLboolean (*UnmapBuffer)(GLenum) = nullptr;

    bool resolve(QOpenGLContext *ctx)
    {
        f = ctx->functions();
        const char *version = reinterpret_cast<const char *>(f->glGetString(GL_VERSION));
        int major = 0;
        if (version)
            std::sscanf(version, "OpenGL ES %d", &major);
        if (major < 3) {
            qWarning() << "ScreenRecorder: needs OpenGL ES 3, context is" << (version ? version : "?");
            return false;
        }
        BlitFramebuffer = reinterpret_cast<decltype(BlitFramebuffer)>(ctx->getProcAddress("glBlitFramebuffer"));
        FenceSync = reinterpret_cast<decltype(FenceSync)>(ctx->getProcAddress("glFenceSync"));
        ClientWaitSync = reinterpret_cast<decltype(ClientWaitSync)>(ctx->getProcAddress("glClientWaitSync"));
        DeleteSync = reinterpret_cast<decltype(DeleteSync)>(ctx->getProcAddress("glDeleteSync"));
        MapBufferRange = reinterpret_cast<decltype(MapBufferRange)>(ctx->getProcAddress("glMapBufferRange"));
        UnmapBuffer = reinterpret_cast<decltype(UnmapBuffer)>(ctx->getProcAddress("glUnmapBuffer"));
        return BlitFramebuffer && FenceSync && ClientWaitSync && DeleteSync && MapBufferRange && UnmapBuffer;
    }
};
Gl3 gl3;   // the shell has one render thread / context

const char kService[] = "com.fp6.ScreenRecorder";
const char kPath[] = "/com/fp6/ScreenRecorder";

qint64 monotonicNs()
{
    timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return qint64(ts.tv_sec) * 1000000000LL + ts.tv_nsec;
}

QString settingsPath()
{
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation)
        + QStringLiteral("/fp6-screen-recorder.conf");
}

// Wire format, little endian, one header per frame followed by
// stride * height bytes of RGBA (top row first).
struct FrameHeader {
    char magic[4];        // "FP6F"
    quint32 version;      // 1
    quint32 width;
    quint32 height;
    quint32 stride;
    quint32 reserved;
    qint64 ptsNs;         // CLOCK_MONOTONIC
};
static_assert(sizeof(FrameHeader) == 32, "frame header layout");

bool writeAll(int fd, const void *data, size_t size)
{
    auto p = static_cast<const char *>(data);
    while (size > 0) {
        ssize_t n = ::send(fd, p, size, MSG_NOSIGNAL);
        if (n < 0) {
            if (errno == EINTR)
                continue;
            return false;
        }
        p += n;
        size -= size_t(n);
    }
    return true;
}

} // namespace

// QProcess that hands our end of the socketpair to the child as fd 3.
class RecorderProcess : public QProcess
{
public:
    explicit RecorderProcess(int fd, QObject *parent) : QProcess(parent), m_fd(fd) {}
protected:
    void setupChildProcess() override
    {
        // async-signal-safe only: we are between fork and exec.
        if (m_fd == 3) {
            int flags = ::fcntl(3, F_GETFD);
            ::fcntl(3, F_SETFD, flags & ~FD_CLOEXEC);
        } else {
            ::dup2(m_fd, 3);   // dup2 clears FD_CLOEXEC on the copy
        }
    }
private:
    int m_fd;
};

ScreenRecorder::ScreenRecorder(QQuickItem *parent)
    : QQuickItem(parent)
{
    QSettings settings(settingsPath(), QSettings::IniFormat);
    m_mic = settings.value(QStringLiteral("recordMicrophone"), false).toBool();
    m_system = settings.value(QStringLiteral("recordSystemAudio"), false).toBool();
    m_intervalNs = 1000000000LL / qMax(1, FP6_RECORD_FPS);
    m_targetWidth = FP6_RECORD_WIDTH;

    m_elapsedTimer.setInterval(1000);
    connect(&m_elapsedTimer, &QTimer::timeout, this, [this] {
        m_elapsed = int((monotonicNs() - m_startedNs) / 1000000000LL);
        Q_EMIT elapsedChanged();
    });
    m_flushTimer.setSingleShot(true);
    connect(&m_flushTimer, &QTimer::timeout, this, [this] {
        if (m_capture && m_window) {
            m_flush = true;
            m_window->update();
        }
    });
}

ScreenRecorder::~ScreenRecorder()
{
    stop();
    // ~QProcess would kill the recorder; let it finish the file first.
    if (m_process && m_process->state() != QProcess::NotRunning && !m_process->waitForFinished(5000))
        qWarning() << "ScreenRecorder: recorder still busy at shell exit";
}

void ScreenRecorder::itemChange(ItemChange change, const ItemChangeData &value)
{
    if (change == ItemSceneChange)
        attachWindow(value.window);
    QQuickItem::itemChange(change, value);
}

void ScreenRecorder::attachWindow(QQuickWindow *window)
{
    if (m_window == window)
        return;
    if (m_window) {
        stop();
        disconnect(m_renderConnection);
    }
    m_window = window;
    if (m_window) {
        m_renderConnection = connect(m_window, &QQuickWindow::afterRendering,
                                     this, &ScreenRecorder::onAfterRendering,
                                     Qt::DirectConnection);
        connect(m_window, &QQuickWindow::sceneGraphInvalidated, this, [this] {
            // render thread, context still current
            releaseGl();
        }, Qt::DirectConnection);
    }
}

void ScreenRecorder::setAllowed(bool allowed)
{
    if (m_allowed == allowed)
        return;
    m_allowed = allowed;
    if (!allowed && (m_recording || m_capture)) {
        // Locked / greeter / display off: never capture what comes next.
        m_capture = false;
        qInfo() << "ScreenRecorder: stopping, the screen is locked or off";
        stop();
    }
    if (allowed)
        exportDBus();
    Q_EMIT allowedChanged();
}

void ScreenRecorder::exportDBus()
{
    static bool exported = false;
    if (exported)
        return;
    exported = true;
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.registerObject(QString::fromLatin1(kPath), this,
                            QDBusConnection::ExportScriptableSlots
                                | QDBusConnection::ExportScriptableSignals)) {
        qWarning() << "ScreenRecorder: cannot export" << kPath;
        return;
    }
    if (!bus.registerService(QString::fromLatin1(kService)))
        qWarning() << "ScreenRecorder: cannot own" << kService << bus.lastError().message();
}

void ScreenRecorder::setRecordMicrophone(bool on)
{
    if (m_mic == on)
        return;
    m_mic = on;
    saveSettings();
    Q_EMIT settingsChanged();
}

void ScreenRecorder::setRecordSystemAudio(bool on)
{
    if (m_system == on)
        return;
    m_system = on;
    saveSettings();
    Q_EMIT settingsChanged();
}

void ScreenRecorder::saveSettings()
{
    QSettings settings(settingsPath(), QSettings::IniFormat);
    settings.setValue(QStringLiteral("recordMicrophone"), m_mic);
    settings.setValue(QStringLiteral("recordSystemAudio"), m_system);
}

void ScreenRecorder::RequestStart()
{
    if (!m_allowed || m_recording || m_saving)
        return;
    Q_EMIT startRequested();
}

void ScreenRecorder::Stop()
{
    stop();
}

bool ScreenRecorder::start(int rotation)
{
    if (!m_allowed || m_recording || m_saving || !m_window) {
        qWarning() << "ScreenRecorder: start refused (allowed" << m_allowed << "recording"
                   << m_recording << "saving" << m_saving << ")";
        return false;
    }
    int fds[2];
    if (::socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, fds) != 0) {
        qWarning() << "ScreenRecorder: socketpair failed" << strerror(errno);
        return false;
    }
    int sndbuf = 8 * 1024 * 1024;
    ::setsockopt(fds[0], SOL_SOCKET, SO_SNDBUF, &sndbuf, sizeof sndbuf);

    rotation = ((rotation % 360) + 360) % 360;
    QStringList args{QStringLiteral("--fd"), QStringLiteral("3"),
                     QStringLiteral("--rotate"), QString::number(rotation),
                     QStringLiteral("--fps"), QString::number(FP6_RECORD_FPS),
                     // Results come back on stdout; the shell posts the
                     // notification (it stays on the bus, the recorder exits).
                     QStringLiteral("--report-stdout")};
    if (m_mic)
        args << QStringLiteral("--microphone");
    if (m_system)
        args << QStringLiteral("--system-audio");

    m_process = new RecorderProcess(fds[1], this);
    m_process->setProcessChannelMode(QProcess::ForwardedErrorChannel);
    connect(m_process, &QProcess::readyReadStandardOutput, this, [this] {
        while (m_process && m_process->canReadLine())
            handleRecorderLine(QString::fromUtf8(m_process->readLine()).trimmed());
    });
    m_process->setInputChannelMode(QProcess::ManagedInputChannel);
    connect(m_process, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this,
            [this](int code, QProcess::ExitStatus status) {
        qInfo() << "ScreenRecorder: recorder exited" << code << status;
        const QStringList rest = QString::fromUtf8(m_process->readAllStandardOutput()).split(QLatin1Char('\n'));
        for (const QString &line : rest)
            handleRecorderLine(line.trimmed());
        if (m_recording)
            stop();
        m_process->deleteLater();
        m_process = nullptr;
        setSaving(false);
    });
    m_process->start(QStringLiteral(FP6_SCREENRECORD_BIN), args);
    ::close(fds[1]);
    if (!m_process->waitForStarted(3000)) {
        qWarning() << "ScreenRecorder: cannot start" << FP6_SCREENRECORD_BIN << m_process->errorString();
        ::close(fds[0]);
        m_process->deleteLater();
        m_process = nullptr;
        return false;
    }

    m_socket = fds[0];
    {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_queue.clear();
        m_writerStop = false;
    }
    m_dropped = 0;
    m_writerDone = false;
    m_writer = std::thread(&ScreenRecorder::writerLoop, this, m_socket);
    m_startedNs = monotonicNs();
    m_elapsed = 0;
    Q_EMIT elapsedChanged();
    m_elapsedTimer.start();
    m_lastCaptureNs = 0;
    m_flush = false;
    m_capture = true;
    setRecording(true);
    setSaving(false);
    m_window->update();
    qInfo() << "ScreenRecorder: recording, rotation" << rotation << "microphone" << m_mic
             << "system audio" << m_system;
    return true;
}

void ScreenRecorder::stop()
{
    m_capture = false;
    m_flushTimer.stop();
    m_elapsedTimer.stop();
    if (m_socket >= 0) {
        {
            std::lock_guard<std::mutex> lock(m_mutex);
            m_writerStop = true;
        }
        m_cond.notify_all();
        // Let the writer flush what is queued, but never hang the shell on a
        // stuck recorder: after 1.5 s the socket is shut down under it.
        for (int i = 0; i < 150 && !m_writerDone; ++i)
            ::usleep(10000);
        if (!m_writerDone)
            ::shutdown(m_socket, SHUT_RDWR);
        if (m_writer.joinable())
            m_writer.join();
        // EOF for the recorder: it finishes the file and exits.
        ::shutdown(m_socket, SHUT_RDWR);
        ::close(m_socket);
        m_socket = -1;
        qInfo() << "ScreenRecorder: stopped," << m_dropped << "frames dropped";
        setRecording(false);
        if (m_process)
            setSaving(true);
    }
    if (m_window) {
        m_releaseGl = true;
        m_window->update();
    }
    setRecording(false);
}

void ScreenRecorder::handleRecorderLine(const QString &line)
{
    const QStringList f = line.split(QLatin1Char('\t'));
    if (f.size() != 4 || f[0] != QLatin1String("FP6-NOTIFY")) {
        if (!line.isEmpty())
            qInfo() << "ScreenRecorder: recorder:" << line;
        return;
    }
    QDBusMessage call = QDBusMessage::createMethodCall(
        QStringLiteral("org.freedesktop.Notifications"), QStringLiteral("/org/freedesktop/Notifications"),
        QStringLiteral("org.freedesktop.Notifications"), QStringLiteral("Notify"));
    call << QStringLiteral("Screen recorder") << 0u << f[1] << f[2] << f[3]
         << QStringList() << QVariantMap() << 5000;
    QDBusConnection::sessionBus().send(call);   // no reply needed
}

void ScreenRecorder::setRecording(bool on)
{
    if (m_recording == on)
        return;
    m_recording = on;
    Q_EMIT recordingChanged();
    Q_EMIT RecordingChanged(on);
}

void ScreenRecorder::setSaving(bool on)
{
    if (m_saving == on)
        return;
    m_saving = on;
    Q_EMIT savingChanged();
}

void ScreenRecorder::armFlush(int ms)
{
    QMetaObject::invokeMethod(this, [this, ms] {
        if (m_capture)
            m_flushTimer.start(ms);
    }, Qt::QueuedConnection);
}

// ---------------------------------------------------------------- render thread

bool ScreenRecorder::ensureGl()
{
    QOpenGLContext *ctx = QOpenGLContext::currentContext();
    if (!ctx || !m_window)
        return false;
    const qreal dpr = m_window->effectiveDevicePixelRatio();
    const int srcW = qRound(m_window->width() * dpr);
    const int srcH = qRound(m_window->height() * dpr);
    if (m_gl.initialised && (m_gl.srcW != srcW || m_gl.srcH != srcH))
        releaseGl();   // output size changed; recorder keeps the first size
    if (m_gl.initialised)
        return true;
    if (m_gl.failed)
        return false;
    if (srcW <= 0 || srcH <= 0)
        return false;
    if (!gl3.resolve(ctx)) {
        qWarning() << "ScreenRecorder: OpenGL ES 3 functions missing - recording disabled";
        m_gl.failed = true;
        return false;
    }
    QOpenGLFunctions *f = gl3.f;
    m_gl.srcW = srcW;
    m_gl.srcH = srcH;
    int w = qMin(m_targetWidth, srcW) & ~1;
    int h = int(qint64(srcH) * w / srcW) & ~1;
    m_gl.dstW = w;
    m_gl.dstH = h;

    GLint prevFbo = 0, prevRbo = 0;
    f->glGetIntegerv(GL_FRAMEBUFFER_BINDING, &prevFbo);
    f->glGetIntegerv(GL_RENDERBUFFER_BINDING, &prevRbo);
    f->glGenFramebuffers(1, &m_gl.fbo);
    f->glGenRenderbuffers(1, &m_gl.rbo);
    f->glBindRenderbuffer(GL_RENDERBUFFER, m_gl.rbo);
    f->glRenderbufferStorage(GL_RENDERBUFFER, GL_RGBA8, w, h);
    f->glBindFramebuffer(GL_FRAMEBUFFER, m_gl.fbo);
    f->glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, m_gl.rbo);
    const GLenum status = f->glCheckFramebufferStatus(GL_FRAMEBUFFER);
    f->glBindFramebuffer(GL_FRAMEBUFFER, prevFbo);
    f->glBindRenderbuffer(GL_RENDERBUFFER, prevRbo);

    GLint prevPbo = 0;
    f->glGetIntegerv(GL_PIXEL_PACK_BUFFER_BINDING, &prevPbo);
    f->glGenBuffers(CaptureGl::kBuffers, m_gl.pbo);
    for (unsigned pbo : m_gl.pbo) {
        f->glBindBuffer(GL_PIXEL_PACK_BUFFER, pbo);
        f->glBufferData(GL_PIXEL_PACK_BUFFER, GLsizeiptr(w) * h * 4, nullptr, GL_STREAM_READ);
    }
    f->glBindBuffer(GL_PIXEL_PACK_BUFFER, prevPbo);
    m_gl.initialised = true;
    if (status != GL_FRAMEBUFFER_COMPLETE) {
        qWarning() << "ScreenRecorder: capture framebuffer incomplete" << Qt::hex << status;
        releaseGl();
        m_gl.failed = true;
        return false;
    }
    qInfo() << "ScreenRecorder: capturing" << srcW << "x" << srcH << "->" << w << "x" << h;
    return true;
}

void ScreenRecorder::releaseGl()
{
    QOpenGLContext *ctx = QOpenGLContext::currentContext();
    if (!m_gl.initialised || !ctx) {
        m_gl = CaptureGl{};
        return;
    }
    QOpenGLFunctions *f = ctx->functions();
    for (int i = 0; i < CaptureGl::kBuffers; ++i) {
        if (m_gl.fence[i] && gl3.DeleteSync)
            gl3.DeleteSync(m_gl.fence[i]);
    }
    f->glDeleteBuffers(CaptureGl::kBuffers, m_gl.pbo);
    f->glDeleteRenderbuffers(1, &m_gl.rbo);
    f->glDeleteFramebuffers(1, &m_gl.fbo);
    m_gl = CaptureGl{};
}

void ScreenRecorder::onAfterRendering()
{
    if (m_releaseGl.exchange(false) && !m_capture) {
        releaseGl();
        return;
    }
    if (!m_capture)
        return;
    if (!ensureGl())
        return;
    QOpenGLFunctions *f = gl3.f;

    // Keep Qt's state: framebuffer/pack-buffer bindings and the scissor test
    // (which would clip the blit).
    GLint prevRead = 0, prevDraw = 0, prevPbo = 0;
    f->glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &prevRead);
    f->glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &prevDraw);
    f->glGetIntegerv(GL_PIXEL_PACK_BUFFER_BINDING, &prevPbo);
    const GLboolean scissor = f->glIsEnabled(GL_SCISSOR_TEST);
    if (scissor)
        f->glDisable(GL_SCISSOR_TEST);

    const qint64 now = monotonicNs();
    const bool flush = m_flush.exchange(false);
    static const bool debug = qEnvironmentVariableIsSet("FP6_SCREENRECORDER_DEBUG");
    if (debug)
        qDebug() << "ScreenRecorder: frame" << now / 1000000 << "flush" << flush
                 << "since last capture" << (now - m_lastCaptureNs) / 1000000;
    collect(flush);
    if (flush) {
        // The screen has been still since the last capture: this frame is the
        // one on screen now; read it synchronously so nothing stays pending.
        captureSync(now);
    } else if (now - m_lastCaptureNs >= m_intervalNs) {
        captureAsync(now);
        armFlush(int(2 * m_intervalNs / 1000000));
    } else {
        // Skipped (frame-rate cap): if this turns out to be the last frame
        // for a while, the flush frame picks it up.
        armFlush(int((2 * m_intervalNs - (now - m_lastCaptureNs)) / 1000000) + 1);
    }

    f->glBindFramebuffer(GL_READ_FRAMEBUFFER, prevRead);
    f->glBindFramebuffer(GL_DRAW_FRAMEBUFFER, prevDraw);
    f->glBindBuffer(GL_PIXEL_PACK_BUFFER, prevPbo);
    if (scissor)
        f->glEnable(GL_SCISSOR_TEST);
}

void ScreenRecorder::captureAsync(qint64 now)
{
    QOpenGLContext *ctx = QOpenGLContext::currentContext();
    QOpenGLFunctions *f = gl3.f;
    const int i = m_gl.next;
    if (m_gl.fence[i]) {
        // Ring full (GPU or writer behind): drop the oldest pending frame.
        gl3.DeleteSync(m_gl.fence[i]);
        m_gl.fence[i] = nullptr;
        ++m_dropped;
    }
    f->glBindFramebuffer(GL_READ_FRAMEBUFFER, ctx->defaultFramebufferObject());
    f->glBindFramebuffer(GL_DRAW_FRAMEBUFFER, m_gl.fbo);
    // Scale and flip (GL rows are bottom-up; the video wants top-down).
    gl3.BlitFramebuffer(0, 0, m_gl.srcW, m_gl.srcH, 0, m_gl.dstH, m_gl.dstW, 0,
                        GL_COLOR_BUFFER_BIT, GL_LINEAR);
    f->glBindFramebuffer(GL_READ_FRAMEBUFFER, m_gl.fbo);
    f->glBindBuffer(GL_PIXEL_PACK_BUFFER, m_gl.pbo[i]);
    f->glPixelStorei(GL_PACK_ALIGNMENT, 4);
    f->glReadPixels(0, 0, m_gl.dstW, m_gl.dstH, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
    m_gl.fence[i] = gl3.FenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0);
    m_gl.pts[i] = now;
    m_gl.next = (i + 1) % CaptureGl::kBuffers;
    m_lastCaptureNs = now;
    const GLenum error = f->glGetError();
    if (error != GL_NO_ERROR) {
        qWarning() << "ScreenRecorder: GL error" << Qt::hex << error << "while capturing - recording disabled";
        m_gl.failed = true;
        m_capture = false;
        QMetaObject::invokeMethod(this, [this] { stop(); }, Qt::QueuedConnection);
    }
}

void ScreenRecorder::collect(bool wait)
{
    QOpenGLFunctions *f = gl3.f;
    // Oldest first: the slot after the last written one.
    for (int k = 0; k < CaptureGl::kBuffers; ++k) {
        const int i = (m_gl.next + k) % CaptureGl::kBuffers;
        if (!m_gl.fence[i])
            continue;
        const GLenum r = gl3.ClientWaitSync(m_gl.fence[i],
                                             wait ? GL_SYNC_FLUSH_COMMANDS_BIT : 0,
                                             wait ? 100000000ULL : 0);
        if (r != GL_ALREADY_SIGNALED && r != GL_CONDITION_SATISFIED)
            break;   // keep the order: later ones are not ready either
        gl3.DeleteSync(m_gl.fence[i]);
        m_gl.fence[i] = nullptr;
        const size_t size = size_t(m_gl.dstW) * m_gl.dstH * 4;
        f->glBindBuffer(GL_PIXEL_PACK_BUFFER, m_gl.pbo[i]);
        void *mapped = gl3.MapBufferRange(GL_PIXEL_PACK_BUFFER, 0, GLsizeiptr(size), GL_MAP_READ_BIT);
        if (!mapped)
            continue;
        CapturedFrame frame;
        frame.ptsNs = m_gl.pts[i];
        frame.width = quint32(m_gl.dstW);
        frame.height = quint32(m_gl.dstH);
        frame.data.assign(static_cast<unsigned char *>(mapped), static_cast<unsigned char *>(mapped) + size);
        gl3.UnmapBuffer(GL_PIXEL_PACK_BUFFER);
        enqueue(std::move(frame));
    }
}

void ScreenRecorder::captureSync(qint64 now)
{
    QOpenGLContext *ctx = QOpenGLContext::currentContext();
    QOpenGLFunctions *f = gl3.f;
    f->glBindFramebuffer(GL_READ_FRAMEBUFFER, ctx->defaultFramebufferObject());
    f->glBindFramebuffer(GL_DRAW_FRAMEBUFFER, m_gl.fbo);
    gl3.BlitFramebuffer(0, 0, m_gl.srcW, m_gl.srcH, 0, m_gl.dstH, m_gl.dstW, 0,
                        GL_COLOR_BUFFER_BIT, GL_LINEAR);
    f->glBindFramebuffer(GL_READ_FRAMEBUFFER, m_gl.fbo);
    f->glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
    f->glPixelStorei(GL_PACK_ALIGNMENT, 4);
    CapturedFrame frame;
    frame.ptsNs = now;
    frame.width = quint32(m_gl.dstW);
    frame.height = quint32(m_gl.dstH);
    frame.data.resize(size_t(m_gl.dstW) * m_gl.dstH * 4);
    f->glReadPixels(0, 0, m_gl.dstW, m_gl.dstH, GL_RGBA, GL_UNSIGNED_BYTE, frame.data.data());
    m_lastCaptureNs = now;
    enqueue(std::move(frame));
}

// ---------------------------------------------------------------- writer thread

void ScreenRecorder::enqueue(CapturedFrame &&frame)
{
    {
        std::lock_guard<std::mutex> lock(m_mutex);
        if (m_writerStop)
            return;
        if (m_queue.size() >= 4) {   // recorder behind: drop the oldest
            m_queue.pop_front();
            ++m_dropped;
        }
        m_queue.push_back(std::move(frame));
    }
    m_cond.notify_one();
}

void ScreenRecorder::writerLoop(int fd)
{
    struct Done {
        std::atomic<bool> &flag;
        ~Done() { flag = true; }
    } done{m_writerDone};
    for (;;) {
        CapturedFrame frame;
        {
            std::unique_lock<std::mutex> lock(m_mutex);
            m_cond.wait(lock, [this] { return m_writerStop || !m_queue.empty(); });
            if (m_writerStop) {
                // Frames still queued at stop are written (at most a few).
                if (m_queue.empty())
                    return;
            }
            frame = std::move(m_queue.front());
            m_queue.pop_front();
        }
        FrameHeader header;
        std::memcpy(header.magic, "FP6F", 4);
        header.version = 1;
        header.width = frame.width;
        header.height = frame.height;
        header.stride = frame.width * 4;
        header.reserved = 0;
        header.ptsNs = frame.ptsNs;
        if (!writeAll(fd, &header, sizeof header) || !writeAll(fd, frame.data.data(), frame.data.size())) {
            qWarning() << "ScreenRecorder: recorder went away:" << strerror(errno);
            std::lock_guard<std::mutex> lock(m_mutex);
            m_queue.clear();
            m_capture = false;
            return;
        }
    }
}
