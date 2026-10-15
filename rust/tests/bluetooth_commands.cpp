#include <QCoreApplication>
#include <QElapsedTimer>
#include <QJsonDocument>
#include <QJSValue>
#include <QPluginLoader>
#include <QProcess>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QTemporaryDir>
#include <QThread>
#include <memory>
#include <cstdio>
#include <functional>
#include "../inir-qt/include/list_model.h"

static bool until(const std::function<bool()> &predicate) {
    QElapsedTimer timer;
    timer.start();
    while (!predicate() && timer.elapsed() < 5000) {
        QCoreApplication::processEvents();
        QThread::msleep(1);
    }
    return predicate();
}

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc != 2) return 2;
    {
        QJSEngine scripts;
        const auto object = scripts.evaluate(R"(({adapter: "hci0", enabled: false,
            files: ["/tmp/first file.txt", "/tmp/second.txt"], nested: {trusted: true}}))");
        const auto parameters = commandVariant(QVariant::fromValue(object)).toMap();
        if (parameters["adapter"].toString() != "hci0"
            || parameters["enabled"].metaType().id() != QMetaType::Bool || parameters["enabled"].toBool()
            || parameters["files"].toList().size() != 2
            || parameters["files"].toList()[0].toString() != "/tmp/first file.txt"
            || !parameters["nested"].toMap()["trusted"].toBool()
            || commandVariant(parameters).toMap() != parameters) {
            fprintf(stderr, "QML command normalization lost strings, booleans or file arrays\n");
            return 1;
        }
    }
    struct PrivateBus {
        QProcess process;
        ~PrivateBus() { process.kill(); process.waitForFinished(); }
    } bus;
    bus.process.start("dbus-daemon", {"--session", "--nofork", "--print-address=1"});
    if (!bus.process.waitForStarted() || !bus.process.waitForReadyRead()) return 2;
    const auto address = bus.process.readLine().trimmed();
    if (!address.startsWith("unix:")) return 2;
    qputenv("DBUS_SYSTEM_BUS_ADDRESS", address);
    qputenv("DBUS_SESSION_BUS_ADDRESS", address);
    qputenv("INIR_NATIVE_DOMAINS", "none");
    QPluginLoader plugin(QString::fromLocal8Bit(argv[1]) + "/qs/services/native/libqs_services_native.so");
    if (!plugin.instance()) { qCritical() << plugin.errorString(); return 1; }
    QQmlEngine engine;
    engine.addImportPath(QString::fromLocal8Bit(argv[1]));
    QQmlComponent component(&engine);
    component.setData(R"(
        import QtQml
        import qs.services.native
        QtObject {
            id: root
            property var controller: NativeBluetoothController
            property var results: ({})
            property bool submitted: false
            property Connections observer: Connections {
                target: root.controller
                function onCommandFinished(id, success, error) {
                    const next = Object.assign({}, root.results)
                    next[id] = error
                    root.results = next
                }
            }
            function run(directory) {
                controller.setEnabled(true, false, directory, false)
                // Neither command can touch hardware: malformed addresses fail
                // validation, and the worker is also confined to a private bus.
                const parameters = {adapter: "hci0", address: "not-an-address"}
                submitted = controller.execute("direct", "connect", parameters)
                    && controller.execute("assigned", "disconnect", Object.assign({}, parameters))
            }
            property bool finished: results.direct !== undefined && results.assigned !== undefined
            property bool valid: results.direct === "Invalid Bluetooth device address"
                && results.assigned === "Invalid Bluetooth device address"
            function stop() { controller.setEnabled(false, false, "", true) }
        }
    )", QUrl("file:///bluetooth-command-test.qml"));
    std::unique_ptr<QObject> root(component.create());
    if (!root) {
        for (const auto &error : component.errors()) fprintf(stderr, "%s\n", qPrintable(error.toString()));
        return 1;
    }
    QTemporaryDir directory;
    QMetaObject::invokeMethod(root.get(), "run", Q_ARG(QVariant, directory.path()));
    const bool finished = until([&] { return root->property("finished").toBool(); });
    const bool valid = root->property("submitted").toBool() && root->property("valid").toBool();
    if (!finished || !valid) fprintf(stderr, "Bluetooth QML parameters were lost: %s\n",
        QJsonDocument::fromVariant(root->property("results").value<QJSValue>().toVariant()).toJson().constData());
    QMetaObject::invokeMethod(root.get(), "stop");
    return finished && valid ? 0 : 1;
}
