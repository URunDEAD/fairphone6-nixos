// SPDX-License-Identifier: GPL-3.0-only
#include "colorfilter.h"

#include <QtGui/QOpenGLContext>
#include <QtGui/QOpenGLFunctions>
#include <QtGui/QOpenGLShaderProgram>
#include <QtGui/QVector3D>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGRenderNode>

#include <atomic>
#include <cmath>
#include <memory>

namespace {

constexpr float kEps = 0.002f;

bool isIdentity(const float rgb[3], float grey, bool hasPre = false)
{
    return !hasPre && grey < kEps && std::fabs(rgb[0] - 1) < kEps && std::fabs(rgb[1] - 1) < kEps
        && std::fabs(rgb[2] - 1) < kEps;
}

const float kIdentity12[12] = { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0 };

const char *kVertex =
    "attribute highp vec2 pos;\n"
    "varying highp vec2 uv;\n"
    "void main() { uv = pos * 0.5 + 0.5; gl_Position = vec4(pos, 0.0, 1.0); }\n";

const char *kBlendFragment =
    "#ifdef GL_ES\nprecision mediump float;\n#endif\n"
    "uniform vec3 colour;\n"
    "void main() { gl_FragColor = vec4(colour, 1.0); }\n";

const char *kFetchFragment =
    "#extension GL_EXT_shader_framebuffer_fetch : require\n"
    "precision mediump float;\n"
    "uniform mat3 m;\n"
    "uniform vec3 o;\n"
    "void main() {\n"
    "    vec4 d = gl_LastFragData[0];\n"
    "    gl_FragColor = vec4(clamp(m * d.rgb + o, 0.0, 1.0), d.a);\n"
    "}\n";

const char *kCopyFragment =
    "#ifdef GL_ES\nprecision mediump float;\n#endif\n"
    "uniform sampler2D tex;\n"
    "uniform mat3 m;\n"
    "uniform vec3 o;\n"
    "varying highp vec2 uv;\n"
    "void main() { gl_FragColor = vec4(clamp(m * texture2D(tex, uv).rgb + o, 0.0, 1.0), 1.0); }\n";

enum Path { PathNone = 0, PathBlend, PathFetch, PathCopy };

QString pathName(int p)
{
    switch (p) {
    case PathBlend: return QStringLiteral("blend");
    case PathFetch: return QStringLiteral("fetch");
    case PathCopy: return QStringLiteral("copy");
    default: return QString();
    }
}

} // namespace

class ColorFilterNode : public QSGRenderNode
{
public:
    explicit ColorFilterNode(std::shared_ptr<std::atomic<int>> lastPath)
        : m_lastPath(std::move(lastPath)) {}
    ~ColorFilterNode() override { releaseResources(); }

    void set(const float rgb[3], float grey, const float *pre, const QRectF &rect)
    {
        for (int i = 0; i < 3; ++i) m_rgb[i] = rgb[i];
        m_grey = grey;
        m_hasPre = pre != nullptr;
        for (int i = 0; i < 12; ++i) m_pre[i] = pre ? pre[i] : kIdentity12[i];
        m_rect = rect;
    }

    StateFlags changedStates() const override
    {
        return BlendState | ScissorState | StencilState | DepthState | ColorState;
    }
    RenderingFlags flags() const override { return BoundedRectRendering; }
    QRectF rect() const override { return m_rect; }

    int lastPath() const { return m_lastPath->load(std::memory_order_relaxed); }

    void releaseResources() override
    {
        if (!m_gl)
            return;
        if (QOpenGLContext::currentContext() == m_ctx) {
            if (m_vbo) m_gl->glDeleteBuffers(1, &m_vbo);
            if (m_tex) m_gl->glDeleteTextures(1, &m_tex);
        }
        m_vbo = m_tex = 0;
        m_blend.reset();
        m_fetch.reset();
        m_copy.reset();
        m_gl = nullptr;
    }

    void render(const RenderState *) override
    {
        QOpenGLContext *ctx = QOpenGLContext::currentContext();
        if (!ctx)
            return;
        if (ctx != m_ctx) {
            releaseResources();
            m_ctx = ctx;
            m_loggedPaths = 0;
            m_gl = ctx->functions();
            m_hasFetch = ctx->isOpenGLES()
                && ctx->hasExtension(QByteArrayLiteral("GL_EXT_shader_framebuffer_fetch"))
                && !qEnvironmentVariableIsSet("FP6_COLORFILTER_NO_FETCH");
            static const GLfloat quad[] = { -1, -1, 1, -1, -1, 1, 1, 1 };
            m_gl->glGenBuffers(1, &m_vbo);
            m_gl->glBindBuffer(GL_ARRAY_BUFFER, m_vbo);
            m_gl->glBufferData(GL_ARRAY_BUFFER, sizeof(quad), quad, GL_STATIC_DRAW);
        }

        const bool diagonal = m_grey < kEps && !m_hasPre;
        float mtx[12];
        ColorFilter::affine(m_rgb, m_grey, m_hasPre ? m_pre : nullptr, mtx);
        // GLSL mat3 uniforms are column-major; mtx is row-major 3x4.
        const GLfloat colMajor[9] = { mtx[0], mtx[4], mtx[8], mtx[1], mtx[5], mtx[9],
                                      mtx[2], mtx[6], mtx[10] };

        m_gl->glDisable(GL_SCISSOR_TEST);
        m_gl->glDisable(GL_STENCIL_TEST);
        m_gl->glDisable(GL_DEPTH_TEST);
        m_gl->glDepthMask(GL_FALSE);
        m_gl->glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);

        QOpenGLShaderProgram *prog = nullptr;
        int path = PathNone;
        if (diagonal) {
            prog = program(m_blend, kBlendFragment);
            path = PathBlend;
        } else if (m_hasFetch) {
            prog = program(m_fetch, kFetchFragment);
            path = PathFetch;
            if (!prog) m_hasFetch = false;   // compile failed: use the copy path from now on
        }
        if (!prog && !diagonal) {
            prog = program(m_copy, kCopyFragment);
            path = PathCopy;
        }
        if (!prog)
            return;

        prog->bind();
        if (path == PathBlend) {
            prog->setUniformValue("colour", QVector3D(m_rgb[0], m_rgb[1], m_rgb[2]));
            m_gl->glEnable(GL_BLEND);
            // rgb: dst * colour; alpha: unchanged.
            m_gl->glBlendFuncSeparate(GL_ZERO, GL_SRC_COLOR, GL_ZERO, GL_ONE);
        } else {
            m_gl->glDisable(GL_BLEND);
            m_gl->glUniformMatrix3fv(prog->uniformLocation("m"), 1, GL_FALSE, colMajor);
            prog->setUniformValue("o", QVector3D(mtx[3], mtx[7], mtx[11]));
            if (path == PathCopy) {
                GLint vp[4];
                m_gl->glGetIntegerv(GL_VIEWPORT, vp);
                m_gl->glActiveTexture(GL_TEXTURE0);
                if (!m_tex) {
                    m_gl->glGenTextures(1, &m_tex);
                    m_texW = m_texH = 0;
                }
                m_gl->glBindTexture(GL_TEXTURE_2D, m_tex);
                if (m_texW != vp[2] || m_texH != vp[3]) {
                    m_texW = vp[2];
                    m_texH = vp[3];
                    m_gl->glTexImage2D(GL_TEXTURE_2D, 0, GL_RGB, m_texW, m_texH, 0, GL_RGB,
                                       GL_UNSIGNED_BYTE, nullptr);
                    m_gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
                    m_gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
                    m_gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
                    m_gl->glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
                }
                m_gl->glCopyTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, vp[0], vp[1], vp[2], vp[3]);
                prog->setUniformValue("tex", 0);
            }
        }

        m_gl->glBindBuffer(GL_ARRAY_BUFFER, m_vbo);
        prog->enableAttributeArray(0);
        prog->setAttributeBuffer(0, GL_FLOAT, 0, 2);
        m_gl->glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        prog->disableAttributeArray(0);
        m_gl->glBindBuffer(GL_ARRAY_BUFFER, 0);
        prog->release();
        m_lastPath->store(path, std::memory_order_relaxed);
        // One line per path and context (journal), no per-frame logging.
        if (!(m_loggedPaths & (1 << path))) {
            m_loggedPaths |= 1 << path;
            qInfo("FP6 ColorFilter: %s path (framebuffer fetch %s)", qPrintable(pathName(path)),
                  m_hasFetch ? "available" : "not available");
        }
    }

private:
    QOpenGLShaderProgram *program(std::unique_ptr<QOpenGLShaderProgram> &slot, const char *frag)
    {
        if (slot)
            return slot->isLinked() ? slot.get() : nullptr;
        slot = std::make_unique<QOpenGLShaderProgram>();
        slot->addShaderFromSourceCode(QOpenGLShader::Vertex, kVertex);
        slot->addShaderFromSourceCode(QOpenGLShader::Fragment, frag);
        slot->bindAttributeLocation("pos", 0);
        if (!slot->link()) {
            qWarning("FP6 ColorFilter: shader link failed: %s", qPrintable(slot->log()));
            return nullptr;
        }
        return slot.get();
    }

    std::shared_ptr<std::atomic<int>> m_lastPath;
    float m_rgb[3] { 1, 1, 1 };
    float m_grey = 0;
    bool m_hasPre = false;
    float m_pre[12] { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0 };
    QRectF m_rect;
    QOpenGLContext *m_ctx = nullptr;
    QOpenGLFunctions *m_gl = nullptr;
    bool m_hasFetch = false;
    int m_loggedPaths = 0;
    GLuint m_vbo = 0;
    GLuint m_tex = 0;
    GLint m_texW = 0, m_texH = 0;
    std::unique_ptr<QOpenGLShaderProgram> m_blend, m_fetch, m_copy;
};

ColorFilter::ColorFilter(QQuickItem *parent)
    : QQuickItem(parent)
{
    setFlag(ItemHasContents, true);
}

void ColorFilter::setChannel(int i, qreal v)
{
    v = qBound<qreal>(0, v, 1);
    if (qFuzzyCompare(m_rgb[i], v))
        return;
    m_rgb[i] = v;
    if (i == 0) Q_EMIT redChanged();
    else if (i == 1) Q_EMIT greenChanged();
    else Q_EMIT blueChanged();
    changed();
}

void ColorFilter::setGreyscale(qreal v)
{
    v = qBound<qreal>(0, v, 1);
    if (qFuzzyCompare(m_grey + 1, v + 1))
        return;
    m_grey = v;
    Q_EMIT greyscaleChanged();
    changed();
}

void ColorFilter::setTemperature(int kelvin)
{
    kelvin = qBound(1000, kelvin, 10000);
    if (kelvin == m_temperature)
        return;
    m_temperature = kelvin;
    const QVector3D c = temperatureToRgb(kelvin);
    setRed(c.x());
    setGreen(c.y());
    setBlue(c.z());
    Q_EMIT temperatureChanged();
}

void ColorFilter::setPreMatrix(const QVariantList &m)
{
    float v[12];
    bool valid = m.size() == 12;
    for (int i = 0; valid && i < 12; ++i) {
        bool ok = false;
        v[i] = float(m.at(i).toDouble(&ok));
        valid = ok && std::isfinite(v[i]);
    }
    bool identity = true;
    for (int i = 0; valid && i < 12; ++i)
        identity = identity && std::fabs(v[i] - kIdentity12[i]) < kEps;
    const bool hasPre = valid && !identity;
    bool same = hasPre == m_hasPre;
    for (int i = 0; same && hasPre && i < 12; ++i)
        same = v[i] == m_pre[i];
    m_preList = m;
    if (same) {
        Q_EMIT preMatrixChanged();
        return;
    }
    m_hasPre = hasPre;
    for (int i = 0; i < 12; ++i)
        m_pre[i] = hasPre ? v[i] : kIdentity12[i];
    Q_EMIT preMatrixChanged();
    changed();
}

void ColorFilter::changed()
{
    const float rgb[3] = { float(m_rgb[0]), float(m_rgb[1]), float(m_rgb[2]) };
    const bool a = !isIdentity(rgb, float(m_grey), m_hasPre);
    if (a != m_active) {
        m_active = a;
        Q_EMIT activeChanged();
    }
    update();
}

void ColorFilter::reportPath(const QString &path)
{
    if (path != m_path) {
        m_path = path;
        Q_EMIT pathChanged();
    }
}

QVector3D ColorFilter::temperatureToRgb(int kelvin)
{
    // Tanner Helland's fit of the CIE 1964 blackbody colours (sRGB, 0..255),
    // normalised to the 6500 K white point.
    auto raw = [](double k, double out[3]) {
        const double t = k / 100.0;
        double r, g, b;
        if (t <= 66) {
            r = 255;
            g = 99.4708025861 * std::log(t) - 161.1195681661;
        } else {
            r = 329.698727446 * std::pow(t - 60, -0.1332047592);
            g = 288.1221695283 * std::pow(t - 60, -0.0755148492);
        }
        if (t >= 66) b = 255;
        else if (t <= 19) b = 0;
        else b = 138.5177312231 * std::log(t - 10) - 305.0447927307;
        out[0] = qBound(0.0, r, 255.0);
        out[1] = qBound(0.0, g, 255.0);
        out[2] = qBound(0.0, b, 255.0);
    };
    double w[3], c[3];
    raw(6500, w);
    raw(qBound(1000, kelvin, 10000), c);
    return QVector3D(qMin(1.0, c[0] / w[0]), qMin(1.0, c[1] / w[1]), qMin(1.0, c[2] / w[2]));
}

void ColorFilter::matrix(const float rgb[3], float grey, float out[9])
{
    // out = diag(rgb) * ((1 - g) I + g L), L rows = Rec. 709 luma weights.
    static const float L[3] = { 0.2126f, 0.7152f, 0.0722f };
    for (int r = 0; r < 3; ++r)
        for (int c = 0; c < 3; ++c)
            out[r * 3 + c] = rgb[r] * ((r == c ? 1 - grey : 0) + grey * L[c]);
}

void ColorFilter::affine(const float rgb[3], float grey, const float *pre, float out[12])
{
    // out = M * pre, M = the 3x3 above, pre = [A | t] (row-major 3x4):
    // M (A x + t) = (M A) x + M t.
    float m[9];
    matrix(rgb, grey, m);
    if (!pre)
        pre = kIdentity12;
    for (int r = 0; r < 3; ++r) {
        for (int c = 0; c < 4; ++c) {
            float sum = 0;
            for (int k = 0; k < 3; ++k)
                sum += m[r * 3 + k] * pre[k * 4 + c];
            out[r * 4 + c] = sum;
        }
    }
}

QSGNode *ColorFilter::updatePaintNode(QSGNode *old, UpdatePaintNodeData *)
{
    auto *node = static_cast<ColorFilterNode *>(old);
    if (node) {
        // Sync phase (GUI thread blocked): pick up the path of the last frame.
        const QString p = pathName(node->lastPath());
        QMetaObject::invokeMethod(this, [this, p] { reportPath(p); }, Qt::QueuedConnection);
    }
    const float rgb[3] = { float(m_rgb[0]), float(m_rgb[1]), float(m_rgb[2]) };
    if (isIdentity(rgb, float(m_grey), m_hasPre) || !isVisible() || width() <= 0 || height() <= 0) {
        // Fully bypassed: no node, nothing drawn.
        delete node;
        if (old)
            QMetaObject::invokeMethod(this, [this] { reportPath(QString()); }, Qt::QueuedConnection);
        return nullptr;
    }
    if (!node)
        node = new ColorFilterNode(std::make_shared<std::atomic<int>>(PathNone));
    node->set(rgb, float(m_grey), m_hasPre ? m_pre : nullptr, QRectF(0, 0, width(), height()));
    node->markDirty(QSGNode::DirtyMaterial);
    return node;
}
