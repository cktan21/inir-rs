#include <QGuiApplication>
#include <QAbstractItemModelTester>
#include <QElapsedTimer>
#include <QPluginLoader>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QSignalSpy>
#include <QThread>
#include <cstdio>
#include <memory>
#include <functional>
static bool until(const std::function<bool()> &predicate) {
    QElapsedTimer t; t.start();
    while (!predicate() && t.elapsed() < 3000) { QCoreApplication::processEvents(); QThread::msleep(1); }
    return predicate();
}
static void require(bool value,const char *message) {
    if (!value) { fprintf(stderr,"%s\n",message); fflush(stderr); std::abort(); }
}
int main(int argc,char **argv) {
    QGuiApplication app(argc,argv);
    if(argc!=2) return 2;
    qputenv("INIR_NATIVE_TEST","1");
    qputenv("INIR_NATIVE_DOMAINS","none"); // fixtures must not contact the compositor
    QPluginLoader plugin(QString::fromLocal8Bit(argv[1])+"/qs/services/native/libqs_services_native.so");
    require(plugin.instance(),qPrintable(plugin.errorString()));
    QQmlEngine engine; engine.addImportPath(QString::fromLocal8Bit(argv[1]));
    QQmlComponent component(&engine);
    component.setData(R"(
        import QtQml
        import QtQml.Models
        import qs.services.native
        QtObject {
            id: root
            property var model: NativeNiri
            property var workspaceModel: NativeWorkspaces
            Component.onCompleted: NativeWorkspaces.setEnabled(true)
            property var collection: []
            property var savedCollection
            property var savedRecord
            property int publications: 0
            property string firstTitle: collection.length ? collection[0].title : ""
            property bool metadataArraysValid: Array.isArray(NativeNiri.jsValue(NativeNiri.keyboardLayouts))
                && Array.isArray(NativeNiri.jsValue(NativeNiri.mruWindowIds))
            property Instantiator rows: Instantiator {
                model: NativeNiri
                onObjectAdded: Qt.callLater(root.publish)
                onObjectRemoved: Qt.callLater(root.publish)
                delegate: QtObject {
                    id: view
                    required property real windowId
                    required property string title
                    property var record: ({ get id() { return view.windowId }, get title() { return view.title } })
                }
            }
            function publish() {
                const next=[]
                for (let i=0;i<rows.count;++i) if(rows.objectAt(i)) next.push(rows.objectAt(i).record)
                collection=next; publications++
            }
            function save() { savedCollection=collection; savedRecord=collection[0] }
            function stable() { return collection===savedCollection && collection[0]===savedRecord }
        }
    )",QUrl("file:///typed-model-test.qml"));
    std::unique_ptr<QObject> root(component.create());
    if(!root) { qCritical()<<component.errors(); return 1; }
    auto *model=qobject_cast<QAbstractItemModel*>(root->property("model").value<QObject*>());
    require(model,"Rust model not exposed as QAbstractItemModel");
    require(root->property("metadataArraysValid").toBool(),"Pre-connect Niri metadata was null instead of an array");
    auto tester=std::make_unique<QAbstractItemModelTester>(model,QAbstractItemModelTester::FailureReportingMode::Fatal);
    auto changed=std::make_unique<QSignalSpy>(model,&QAbstractItemModel::dataChanged);
    auto resets=std::make_unique<QSignalSpy>(model,&QAbstractItemModel::modelReset);
    auto fixture=[&](int count,int row,const QString &title){
        const auto calls=model->property("applyCalls").toULongLong();
        bool accepted=false;
        QMetaObject::invokeMethod(model,"fixture",Q_RETURN_ARG(bool,accepted),Q_ARG(int,count),Q_ARG(int,row),Q_ARG(QString,title));
        require(accepted,"Synthetic fixture rejected");
        require(until([&]{return model->property("applyCalls").toULongLong()>calls;}),"Worker delta did not reach Qt");
        QCoreApplication::processEvents();
    };
    fixture(100,-1,"seed");
    auto *workspaceModel=root->property("workspaceModel").value<QObject*>();
    require(workspaceModel,"Workspace model missing");
    require(until([&]{return workspaceModel->property("ready").toBool();}),"Workspace readiness did not propagate");
    const auto workspaceCalls=workspaceModel->property("applyCalls").toULongLong();
    require(until([&]{return root->property("collection").value<QJSValue>().property("length").toInt()==100;}),"Role views missing");
    QMetaObject::invokeMethod(root.get(),"save");
    const int publications=root->property("publications").toInt();
    changed->clear();
    fixture(100,0,"changed");
    require(changed->count()==1,"Title event touched more than one row");
    const auto roleNames=model->roleNames();
    const int titleRole=roleNames.key("title");
    require((*changed)[0][2].value<QList<int>>()==QList<int>{titleRole},"Title role mask was not narrow");
    require(root->property("firstTitle").toString()=="changed","Role-backed getter did not update frontend binding");
    QVariant stable;
    QMetaObject::invokeMethod(root.get(),"stable",Q_RETURN_ARG(QVariant,stable));
    require(stable.toBool()&&root->property("publications").toInt()==publications,"Title update republished the collection/row");
    QElapsedTimer settled; settled.start();
    while(settled.elapsed()<100) { QCoreApplication::processEvents(); QThread::msleep(1); }
    require(workspaceModel->property("applyCalls").toULongLong()==workspaceCalls,
        "Title-only publication queued an empty workspace GUI apply");
    fixture(100,-2,"unused");
    require(model->data(model->index(0,0),roleNames.key("windowId")).toDouble()==99,"Move edits lost ordering");
    fixture(99,-1,"small");
    fixture(100,-1,"restored");
    require(resets->isEmpty(),"Steady-state edits reset the model");
    // Remove test instrumentation; keep the 100 QML role views attached.
    tester.reset();
    changed.reset();
    resets.reset();
    const auto beforeCalls=model->property("applyCalls").toULongLong();
    const double beforeUs=model->property("applyTotalUs").toDouble();
    for(int i=0;i<100;++i) fixture(100,0,"title-"+QString::number(i));
    const double one=(model->property("applyTotalUs").toDouble()-beforeUs)/(model->property("applyCalls").toULongLong()-beforeCalls);
    const auto bulkCalls=model->property("applyCalls").toULongLong();
    const double bulkUs=model->property("applyTotalUs").toDouble();
    for(int i=0;i<100;++i) fixture(100,-1,"all-"+QString::number(i));
    const double bulk=(model->property("applyTotalUs").toDouble()-bulkUs)/(model->property("applyCalls").toULongLong()-bulkCalls);
    printf("{\"rows\":100,\"oneRowApplyMeanUs\":%.3f,\"allRowsApplyMeanUs\":%.3f,\"qmlRoleViewsAttached\":true}\n",one,bulk);
    fflush(stdout);
    require(one<50,"Phase 2 single-row/100-row-model apply gate failed");
    qInfo("Typed model inserts/removes/moves, role masks and stable frontend row identity passed");
    return 0;
}
