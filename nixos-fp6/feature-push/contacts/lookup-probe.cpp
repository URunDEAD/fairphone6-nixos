// Test helper (not installed): resolve one phone number through
// lomiri-history-service's ContactMatcher, the code the history daemon and the
// apps' Lomiri.History models use. Prints only the probe's own result.
// Usage: fp6-contacts-lookup-probe NUMBER [ACCOUNT_ID]
#include <QCoreApplication>
#include <QTimer>
#include <QTextStream>
#include <QContactManager>
#include "contactmatcher_p.h"

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (argc < 2) {
        return 2;
    }
    const QString number = QString::fromLocal8Bit(argv[1]);
    const QString account = argc > 2 ? QString::fromLocal8Bit(argv[2]) : QStringLiteral("ofono/ofono/account0");
    QTextStream out(stdout);
    out << "LOMIRI_CONTACT_BACKEND=" << qEnvironmentVariable("LOMIRI_CONTACT_BACKEND")
        << " available=" << QContactManager::availableManagers().join(',') << Qt::endl;
    History::ContactMatcher *matcher = History::ContactMatcher::instance();
    // Let TelepathyHelper find the account (addressable fields: tel).
    QTimer::singleShot(4000, [&]() {
        const QVariantMap info = matcher->contactInfo(account, number, true);
        out << "RESULT alias=" << info.value("alias").toString()
            << " contactId=" << (info.value("contactId").toString().isEmpty() ? "none" : "found") << Qt::endl;
        app.quit();
    });
    return app.exec();
}
