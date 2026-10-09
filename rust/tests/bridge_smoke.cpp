#include <QCoreApplication>
#include <QElapsedTimer>
#include <QFile>
#include <QDir>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPluginLoader>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QTemporaryDir>
#include <QThread>
#include <functional>
#include <cstdio>
#include <memory>

static bool until(const std::function<bool()> &predicate, int timeout = 4000) {
    QElapsedTimer timer;
    timer.start();
    while (!predicate() && timer.elapsed() < timeout) {
        QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
        QThread::msleep(5);
    }
    return predicate();
}

static bool check(bool result, const char *message) {
    if (!result) fprintf(stderr, "%s\n", message);
    return result;
}

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc != 2) return 2;
    // Loading the library's constructors alone can register QML types even
    // when a Rust cdylib has hidden Qt's plugin entry points. Quickshell also
    // requires a real plugin instance, so exercise that path explicitly.
    QPluginLoader plugin(QString::fromLocal8Bit(argv[1])
        + "/qs/services/native/libqs_services_native.so");
    if (!plugin.instance()) {
        qCritical() << "Native plugin instance could not load:" << plugin.errorString();
        return 1;
    }
    const bool liveServices = qEnvironmentVariableIsSet("INIR_TEST_LIVE_SERVICES");
    QTemporaryDir directory;
    const QString path = directory.filePath("config.json");
    QFile initial(path);
    if (!initial.open(QIODevice::WriteOnly)) return 2;
    initial.write(R"({"legacy":{"preserved":true},"performance":{"blurBackend":"auto"}})");
    initial.close();

    // Never link the plugin into the test executable. Importing exercises the
    // same dynamic lookup path a Quickshell process uses.
    int warmThreads = 0;
    for (int reload = 0; reload < 25; ++reload) {
        QQmlEngine engine;
        engine.addImportPath(QString::fromLocal8Bit(argv[1]));
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQml
            import qs.services.native
            QtObject {
                id: root
                property bool ready: SystemInfo.ready
                property string hostname: SystemInfo.hostname
                property real uptime: SystemInfo.uptime
                property real memory: SystemInfo.memoryTotal
                property bool cpuAvailable: SystemInfo.cpuUsageAvailable
                property bool configReady: ConfigService.ready
                property string document: ConfigService.documentJson
                property string error: ConfigService.errorString
                property int writeCount: 0
                property bool desktopActive: DesktopServices.active
                property int accessPointCount: DesktopServices.accessPoints.count
                // Phase 1 efficiency revamp: only network and power are served
                // natively. Bluetooth/battery/audio/media are served by
                // Quickshell's existing C++/QML and never report ready here.
                property bool servicesReady: DesktopServices.networkReady && DesktopServices.powerReady
                function activateServices(active) { DesktopServices.setServicesActive(active) }
                property int commandFailures: 0
                property string commandIds: ""
                property Connections commandObserver: Connections {
                    target: DesktopServices
                    function onCommandFinished(id, success, error) {
                        if (!success && error.length > 0) root.commandFailures++
                        if (id.startsWith("fifo-")) root.commandIds += id + ","
                    }
                }
                function invalidCommand() { DesktopServices.execute("bad", '{"type":"invalid"}') }
                function queueInactiveCommands() {
                    for (let i = 0; i < 32; ++i) DesktopServices.execute("fifo-" + i, '{"type":"wifiScan"}')
                }
                property Connections writeObserver: Connections {
                    target: ConfigService
                    function onWriteFinished(key, success) {
                        if (success) root.writeCount++
                    }
                }
                function monitor(active) { SystemInfo.setResourceMonitoring(active) }
                function open(path) { ConfigService.open(path) }
                function patch() { ConfigService.setValue("performance.lowPower", "true") }
                function patchOrder() {
                    for (let i = 0; i < 32; ++i)
                        ConfigService.setValue("extension.sequence", JSON.stringify(i))
                }
            }
        )", QUrl("file:///bridge-smoke.qml"));
        std::unique_ptr<QObject> object(component.create());
        if (!object) { qCritical() << component.errors(); return 1; }
        if (!check(!object->property("desktopActive").toBool() && object->property("accessPointCount").toInt() == 0, "desktop services started without a consumer")) return 1;
        QMetaObject::invokeMethod(object.get(), "invalidCommand");
        if (!check(object->property("commandFailures").toInt() == 1, "invalid command did not report an error")) return 1;
        if (!check(until([&] { return object->property("ready").toBool() && object->property("memory").toDouble() > 0; }), "SystemInfo never became ready")) return 1;
        if (!check(!object->property("hostname").toString().isEmpty(), "hostname is empty")) return 1;
        if (reload == 0) {
            // The consumer is inactive, so these requests exercise FIFO/error
            // completion without issuing any command to real desktop services.
            QMetaObject::invokeMethod(object.get(), "queueInactiveCommands");
            QString expectedIds;
            for (int i = 0; i < 32; ++i) expectedIds += "fifo-" + QString::number(i) + ",";
            if (!check(until([&] { return object->property("commandIds").toString() == expectedIds; }), "service commands were dropped or reordered")) return 1;
            if (liveServices) {
                QMetaObject::invokeMethod(object.get(), "activateServices", Q_ARG(QVariant, true));
                // Phase 1: network and power are the natively served live domains.
                if (!check(until([&] { return object->property("servicesReady").toBool(); }, 15000), "live desktop services never reached Qt")) return 1;
                QMetaObject::invokeMethod(object.get(), "activateServices", Q_ARG(QVariant, false));
            }
            const double uptime = object->property("uptime").toDouble();
            QMetaObject::invokeMethod(object.get(), "monitor", Q_ARG(QVariant, true));
            if (!check(until([&] { return object->property("uptime").toDouble() > uptime; }), "worker updates did not reach Qt")) return 1;
            if (!check(until([&] { return object->property("cpuAvailable").toBool(); }), "periodic sampling did not produce a CPU delta")) return 1;
            QMetaObject::invokeMethod(object.get(), "monitor", Q_ARG(QVariant, false));
            QMetaObject::invokeMethod(object.get(), "open", Q_ARG(QVariant, path));
            if (!check(until([&] { return object->property("configReady").toBool(); }), "configuration failed to load")) return 1;
            QMetaObject::invokeMethod(object.get(), "patch");
            if (!check(until([&] {
                return QJsonDocument::fromJson(object->property("document").toString().toUtf8()).object()["performance"].toObject()["lowPower"].toBool();
            }), "configuration patch failed")) return 1;
            const QJsonObject saved = QJsonDocument::fromJson(object->property("document").toString().toUtf8()).object();
            if (!check(saved["legacy"].toObject()["preserved"].toBool(), "migration lost legacy data")) return 1;
            QMetaObject::invokeMethod(object.get(), "patchOrder");
            if (!check(until([&] { return object->property("writeCount").toInt() == 33; }), "queued configuration writes did not complete")) return 1;
            if (!check(QJsonDocument::fromJson(object->property("document").toString().toUtf8()).object()["extension"].toObject()["sequence"].toInt() == 31, "rapid configuration writes were reordered")) return 1;
            QFile backup(path + ".bak");
            if (!check(backup.exists(), "migration backup is missing")) return 1;
            // Editor-style atomic replacement must reach the live QObject.
            QFile replacement(directory.filePath("replacement.json"));
            if (!replacement.open(QIODevice::WriteOnly)) return 2;
            replacement.write(R"({"schema_version":1,"performance":{"lowPower":false}})");
            replacement.close();
            QFile::remove(path);
            QFile::rename(replacement.fileName(), path);
            if (!check(until([&] {
                return !QJsonDocument::fromJson(object->property("document").toString().toUtf8()).object()["performance"].toObject()["lowPower"].toBool();
            }), "atomic replacement did not reload")) return 1;
            const QString validDocument = object->property("document").toString();
            QFile invalid(path);
            if (!invalid.open(QIODevice::WriteOnly | QIODevice::Truncate)) return 2;
            invalid.write("{");
            invalid.close();
            if (!check(until([&] { return !object->property("error").toString().isEmpty(); }), "invalid external config was not reported")) return 1;
            if (!check(object->property("document").toString() == validDocument, "invalid config replaced the last valid snapshot")) return 1;
            if (!invalid.open(QIODevice::WriteOnly | QIODevice::Truncate)) return 2;
            invalid.write(validDocument.toUtf8());
            invalid.close();
            if (!check(until([&] { return object->property("error").toString().isEmpty(); }), "configuration did not recover after a valid external edit")) return 1;
        }
        // Leave a queued worker callback outstanding while destroying engines.
        QMetaObject::invokeMethod(object.get(), "monitor", Q_ARG(QVariant, true));
        if (reload == 5) warmThreads = QDir("/proc/self/task").entryList(QDir::Dirs | QDir::NoDotAndDotDot).size();
    }
    QCoreApplication::processEvents();
    if (!check(until([&] {
        return QDir("/proc/self/task").entryList(QDir::Dirs | QDir::NoDotAndDotDot).size() <= warmThreads;
    }), "worker threads accumulated across engine reloads")) return 1;
    qInfo("Dynamic import, worker updates, config writes/reload and 25 engine reloads passed");
    return 0;
}
