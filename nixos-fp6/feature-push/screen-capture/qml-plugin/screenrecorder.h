// SPDX-License-Identifier: GPL-3.0-only
// FP6 screen recorder: the Lomiri side (QML item living in the shell scene).
//
// Lomiri composites the whole shell (panel, launcher, app surfaces) with the
// Qt scene graph; Mir only sees the app surfaces, so Mir's own screen
// shooter (wlr-screencopy) cannot capture what is on screen. This item reads
// back the frame Lomiri has just rendered (QQuickWindow::afterRendering, on
// the render thread): a scaled, vertically flipped glBlitFramebuffer into a
// small RGBA renderbuffer, then glReadPixels into a pixel-pack buffer that is
// collected a frame later (no GPU stall), and hands the RGBA frames over a
// socketpair to its child process fp6-screenrecord (Rust/GStreamer), which
// encodes them. Nothing else can get at the frames: there is no socket
// name, no Wayland global and no D-Bus method that delivers pixels.
#pragma once

#include <QObject>
#include <QProcess>
#include <QQuickItem>
#include <QTimer>
#include <QVariantMap>

#include <atomic>
#include <condition_variable>
#include <deque>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

class QQuickWindow;
class RecorderProcess;

struct CapturedFrame {
    qint64 ptsNs = 0;
    quint32 width = 0, height = 0;
    std::vector<unsigned char> data;
};

// GL state; only touched on the render thread.
struct CaptureGl {
    bool initialised = false;
    bool failed = false;
    int srcW = 0, srcH = 0, dstW = 0, dstH = 0;
    unsigned fbo = 0, rbo = 0;
    static const int kBuffers = 3;
    unsigned pbo[kBuffers] = {0, 0, 0};
    void *fence[kBuffers] = {nullptr, nullptr, nullptr};
    qint64 pts[kBuffers] = {0, 0, 0};
    int next = 0;
};

class ScreenRecorder : public QQuickItem
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "com.fp6.ScreenRecorder")

    // Set by the shell: false on the greeter, while locked, with the display
    // off. Becoming false stops a running recording at once.
    Q_PROPERTY(bool allowed READ allowed WRITE setAllowed NOTIFY allowedChanged)
    Q_PROPERTY(bool recording READ recording NOTIFY recordingChanged)
    // true from stop() until the file is written.
    Q_PROPERTY(bool saving READ saving NOTIFY savingChanged)
    Q_PROPERTY(int elapsed READ elapsed NOTIFY elapsedChanged)
    // Persisted in ~/.config/fp6-screen-recorder.conf.
    Q_PROPERTY(bool recordMicrophone READ recordMicrophone WRITE setRecordMicrophone NOTIFY settingsChanged)
    Q_PROPERTY(bool recordSystemAudio READ recordSystemAudio WRITE setRecordSystemAudio NOTIFY settingsChanged)

public:
    explicit ScreenRecorder(QQuickItem *parent = nullptr);
    ~ScreenRecorder() override;

    bool allowed() const { return m_allowed; }
    void setAllowed(bool allowed);
    bool recording() const { return m_recording; }
    bool saving() const { return m_saving; }
    int elapsed() const { return m_elapsed; }
    bool recordMicrophone() const { return m_mic; }
    void setRecordMicrophone(bool on);
    bool recordSystemAudio() const { return m_system; }
    void setRecordSystemAudio(bool on);

    // QML: start after the user confirmed. rotation = the shell's
    // transformRotationAngle (0/90/180/270) so the video is upright.
    Q_INVOKABLE bool start(int rotation);
    Q_INVOKABLE void stop();

public Q_SLOTS:
    // D-Bus (com.fp6.ScreenRecorder on /com/fp6/ScreenRecorder). RequestStart
    // only asks the shell to show its confirmation dialog: a recording never
    // starts without a tap in the shell itself.
    Q_SCRIPTABLE void RequestStart();
    Q_SCRIPTABLE void Stop();
    Q_SCRIPTABLE bool IsRecording() const { return m_recording; }

Q_SIGNALS:
    void allowedChanged();
    void recordingChanged();
    void savingChanged();
    void elapsedChanged();
    void settingsChanged();
    // For QML (confirmation dialog).
    void startRequested();
    // D-Bus signal.
    Q_SCRIPTABLE void RecordingChanged(bool recording);

protected:
    void itemChange(ItemChange change, const ItemChangeData &value) override;

private:
    void attachWindow(QQuickWindow *window);
    void onAfterRendering();          // render thread
    void captureAsync(qint64 now);    // render thread
    void collect(bool wait);          // render thread
    void captureSync(qint64 now);     // render thread
    bool ensureGl();                  // render thread
    void releaseGl();                 // render thread
    void enqueue(CapturedFrame &&frame);
    void writerLoop(int fd);
    void armFlush(int ms);
    void handleRecorderLine(const QString &line);
    void setRecording(bool on);
    void setSaving(bool on);
    void saveSettings();
    void exportDBus();

    QQuickWindow *m_window = nullptr;
    QMetaObject::Connection m_renderConnection;
    bool m_allowed = false;
    bool m_recording = false;
    bool m_saving = false;
    bool m_mic = false;
    bool m_system = false;
    int m_elapsed = 0;
    QTimer m_elapsedTimer;
    QTimer m_flushTimer;
    qint64 m_startedNs = 0;

    // shared with the render thread
    std::atomic<bool> m_capture{false};
    std::atomic<bool> m_flush{false};
    std::atomic<bool> m_releaseGl{false};
    std::atomic<qint64> m_lastCaptureNs{0};
    qint64 m_intervalNs = 1000000000LL / 30;
    int m_targetWidth = 720;
    CaptureGl m_gl;

    // writer thread
    std::mutex m_mutex;
    std::condition_variable m_cond;
    std::deque<CapturedFrame> m_queue;
    bool m_writerStop = false;
    std::atomic<bool> m_writerDone{true};
    std::thread m_writer;
    int m_socket = -1;
    RecorderProcess *m_process = nullptr;
    quint64 m_dropped = 0;
};
