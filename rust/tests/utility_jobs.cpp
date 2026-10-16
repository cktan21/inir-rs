#include <QCoreApplication>
#include <QElapsedTimer>
#include <QFile>
#include <QDir>
#include <QPluginLoader>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QTemporaryDir>
#include <QThread>
#include <cstdio>
#include <functional>
#include <memory>

static bool until(const std::function<bool()> &predicate) {
    QElapsedTimer timer; timer.start();
    while (!predicate() && timer.elapsed() < 5000) {
        QCoreApplication::processEvents(); QThread::msleep(1);
    }
    return predicate();
}
static void require(bool condition, const char *message) {
    if (!condition) { fprintf(stderr, "%s\n", message); std::abort(); }
}
int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc != 2) return 2;
    qputenv("INIR_NATIVE_DOMAINS", "none");
    qputenv("INIR_RUST_BACKEND", "0");
    QPluginLoader plugin(QString::fromLocal8Bit(argv[1]) + "/qs/services/native/libqs_services_native.so");
    require(plugin.instance(), qPrintable(plugin.errorString()));
    QTemporaryDir directory;
    require(directory.isValid(), "Cannot create utility fixture directory");
    QFile fixture(directory.filePath("fixture.txt"));
    require(fixture.open(QIODevice::WriteOnly), "Cannot write utility fixture");
    fixture.write("exact\n\n"); fixture.close();
    const auto threadsBefore = QDir("/proc/self/task").entryList(QDir::Dirs | QDir::NoDotAndDotDot).size();
    for (int cycle = 0; cycle < 10; ++cycle) {
        QQmlEngine engine; engine.addImportPath(QString::fromLocal8Bit(argv[1]));
        engine.rootContext()->setContextProperty("fixturePath", fixture.fileName());
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQml
            import qs.services.native
            QtObject {
                id: root
                property int completed: 0
                property bool valid: true
                property var jobs: NativeUtilityJobs
                property Connections results: Connections {
                    target: NativeUtilityJobs
                    function onFinished(id, success, value, error) {
                        const result = NativeUtilityJobs.jsValue(value)
                        if (id === "read") root.valid = root.valid && success
                            && Array.isArray(result) && result[0].text === "exact\n\n"
                        else if (id === "cancel") root.valid = root.valid && !success && error.includes("canceled")
                        else if (id === "lyrics") root.valid = root.valid && success && result.status === "no_info"
                        else root.valid = false
                        root.completed++
                    }
                }
                Component.onCompleted: {
                    valid = NativeUtilityJobs.ready
                        && NativeUtilityJobs.submit("read", "files.read", {paths: [fixturePath]})
                        && !NativeUtilityJobs.submit("read", "files.read", {paths: []})
                        && !NativeUtilityJobs.submit("bad", "not.an.operation", {})
                        && NativeUtilityJobs.submit("cancel", "lyrics.fetch", {title: "unused", artist: "unused", api: "http://127.0.0.1:1"})
                        && NativeUtilityJobs.cancel("cancel")
                        && !NativeUtilityJobs.cancel("missing")
                        && NativeUtilityJobs.submit("lyrics", "lyrics.fetch", {})
                }
            }
        )", QUrl("file:///utility-jobs-test.qml"));
        std::unique_ptr<QObject> root(component.create());
        if (!root) { qCritical() << component.errors(); return 1; }
        require(until([&] { return root->property("completed").toInt() == 3; }), "Utility jobs did not settle");
        require(root->property("valid").toBool(), "Utility payload, duplicate or cancellation contract failed");
        auto *jobs = root->property("jobs").value<QObject*>();
        require(jobs && jobs->property("pendingCount").toInt() == 0, "Utility tasks leaked after completion");
    }
    require(until([&] { return QDir("/proc/self/task").entryList(QDir::Dirs | QDir::NoDotAndDotDot).size() <= threadsBefore + 1; }), "Utility engine teardown leaked worker threads");
    qInfo("Utility jobs work without the desktop-backend toggle; payloads, cancellation and engine teardown passed");
}
