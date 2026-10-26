// Hardware-free check: the fp6 backend survives iio-sensor-proxy restarts.
// Runs inside dbus-run-session with DBUS_SYSTEM_BUS_ADDRESS pointing at it.
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusInterface>
#include <QDBusReply>
#include <QElapsedTimer>
#include <QOrientationSensor>
#include <QProcess>
#include <QThread>
#include <cstdio>
#include <functional>

static QStringList fakeCmd;
static int failures = 0;

static bool waitFor(const std::function<bool()> &cond, int ms = 8000)
{
    QElapsedTimer t; t.start();
    while (t.elapsed() < ms) {
        QCoreApplication::processEvents(QEventLoop::AllEvents, 50);
        if (cond()) return true;
        QThread::msleep(20);
    }
    return cond();
}
static void check(bool ok, const char *what)
{
    std::printf("%s %s\n", ok ? "ok  " : "FAIL", what);
    std::fflush(stdout);
    if (!ok) ++failures;
}
static int claims()
{
    QDBusInterface i("net.hadess.SensorProxy", "/net/hadess/SensorProxy",
                     "net.hadess.SensorProxy", QDBusConnection::systemBus());
    QDBusReply<int> r = i.call("Claims");
    return r.isValid() ? r.value() : -1;
}
static QProcess *startFake(const QString &orientation, bool late)
{
    auto *p = new QProcess;
    p->setProcessChannelMode(QProcess::ForwardedChannels);
    QStringList args = fakeCmd.mid(1);
    args << orientation;
    if (late) args << "--late";
    p->start(fakeCmd.first(), args);
    p->waitForStarted();
    return p;
}
static void killFake(QProcess *p)
{
    p->kill();
    p->waitForFinished();
    delete p;
}

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    for (int i = 1; i < argc; ++i) fakeCmd << QString::fromLocal8Bit(argv[i]);

    QOrientationSensor sensor;
    sensor.setIdentifier("fp6-iio-sensor-proxy.orientationsensor");
    auto orientation = [&]() {
        return sensor.reading() ? sensor.reading()->orientation() : QOrientationReading::Undefined;
    };
    check(sensor.connectToBackend(), "backend registered without a running proxy");
    sensor.start();
    check(sensor.isActive(), "sensor stays active while the proxy is absent");

    QProcess *a = startFake("normal", false);
    check(waitFor([&] { return claims() == 1 && orientation() == QOrientationReading::TopUp; }),
          "claims once the proxy appears (TopUp)");
    QDBusInterface ia("net.hadess.SensorProxy", "/net/hadess/SensorProxy",
                      "net.hadess.SensorProxy", QDBusConnection::systemBus());
    ia.call("Inject", "AccelerometerOrientation", QVariant::fromValue(QDBusVariant("left-up")));
    check(waitFor([&] { return orientation() == QOrientationReading::LeftUp; }),
          "follows AccelerometerOrientation (LeftUp)");

    killFake(a);
    waitFor([] { return !QDBusConnection::systemBus().interface()->isServiceRegistered("net.hadess.SensorProxy"); });
    QCoreApplication::processEvents();
    check(sensor.isActive(), "still active after the proxy exits");

    QProcess *b = startFake("right-up", true);
    check(waitFor([&] { return claims() == 1 && orientation() == QOrientationReading::RightUp; }),
          "re-claims a restarted proxy once HasAccelerometer turns true (RightUp)");
    QDBusInterface ib("net.hadess.SensorProxy", "/net/hadess/SensorProxy",
                      "net.hadess.SensorProxy", QDBusConnection::systemBus());
    ib.call("Inject", "AccelerometerOrientation", QVariant::fromValue(QDBusVariant("bottom-up")));
    check(waitFor([&] { return orientation() == QOrientationReading::TopDown; }),
          "readings flow from the new proxy (TopDown)");

    sensor.stop();
    check(!sensor.isActive() && waitFor([] { return claims() == 0; }), "stop() releases the claim");
    killFake(b);

    std::printf(failures ? "reclaim-test: %d failure(s)\n" : "reclaim-test: all checks passed\n", failures);
    return failures ? 1 : 0;
}
