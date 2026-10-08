#include "inir-qt/service_models.h"
#include <QAbstractItemModelTester>
#include <QCoreApplication>
#include <QPersistentModelIndex>
#include <cstdio>

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    RecordModel model({"id", "title", "focused"});
    QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::Fatal);
    int resets = 0, changed = 0, moves = 0;
    QObject::connect(&model, &QAbstractItemModel::modelReset, [&] { ++resets; });
    QObject::connect(&model, &QAbstractItemModel::dataChanged, [&] { ++changed; });
    QObject::connect(&model, &QAbstractItemModel::rowsMoved, [&] { ++moves; });
    auto check = [](bool result, const char *message) { if (!result) fprintf(stderr, "%s\n", message); return result; };
    if (!model.apply(R"([{"id":"a","title":"A"},{"id":"b","title":"B"},{"id":"c","title":"C"}])")) return 1;
    QPersistentModelIndex kept(model.index(1));
    if (!model.apply(R"([{"id":"b","title":"Updated"},{"id":"c","title":"C"},{"id":"d","title":"D"}])")) return 1;
    if (!check(kept.isValid() && kept.row() == 0 && model.get(0).value("id") == "b", "surviving device lost its persistent index")) return 1;
    if (!check(changed == 1 && resets == 0 && model.count() == 3, "incremental update reset the model or changed untouched rows")) return 1;
    if (!model.apply(R"([{"id":"d","title":"D"},{"id":"b","title":"Updated"},{"id":"c","title":"C"}])")) return 1;
    if (!check(kept.row() == 1 && moves == 1, "reordering did not move the surviving row")) return 1;
    if (!check(!model.apply(R"([{"id":"b"},{"id":"b"}])") && model.count() == 3, "duplicate keys corrupted the model")) return 1;
    if (!check(!model.apply("broken") && kept.isValid(), "malformed update corrupted the model")) return 1;
    if (!model.apply("[]")) return 1;
    return check(!kept.isValid() && model.count() == 0 && resets == 0, "clear did not remove rows incrementally") ? 0 : 1;
}
