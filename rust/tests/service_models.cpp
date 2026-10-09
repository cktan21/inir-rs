#include "inir-qt/service_models.h"
#include <QAbstractItemModelTester>
#include <QCoreApplication>
#include <QPersistentModelIndex>
#include <QVariantList>
#include <QVariantMap>
#include <cstdio>

// Rows now arrive as typed QVariantMaps built in Rust (no JSON), so the test
// exercises applyRows directly with the same insert/move/change/clear paths.
static QVariant row(const QString &id, const QString &title) {
    QVariantMap m;
    m.insert("id", id);
    if (!title.isNull()) m.insert("title", title);
    return m;
}

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    RecordModel model({"id", "title", "focused"});
    QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::Fatal);
    int resets = 0, changed = 0, moves = 0;
    QObject::connect(&model, &QAbstractItemModel::modelReset, [&] { ++resets; });
    QObject::connect(&model, &QAbstractItemModel::dataChanged, [&] { ++changed; });
    QObject::connect(&model, &QAbstractItemModel::rowsMoved, [&] { ++moves; });
    auto check = [](bool result, const char *message) { if (!result) fprintf(stderr, "%s\n", message); return result; };
    if (!model.applyRows({row("a", "A"), row("b", "B"), row("c", "C")})) return 1;
    QPersistentModelIndex kept(model.index(1));
    if (!model.applyRows({row("b", "Updated"), row("c", "C"), row("d", "D")})) return 1;
    if (!check(kept.isValid() && kept.row() == 0 && model.get(0).value("id") == "b", "surviving device lost its persistent index")) return 1;
    if (!check(changed == 1 && resets == 0 && model.count() == 3, "incremental update reset the model or changed untouched rows")) return 1;
    if (!model.applyRows({row("d", "D"), row("b", "Updated"), row("c", "C")})) return 1;
    if (!check(kept.row() == 1 && moves == 1, "reordering did not move the surviving row")) return 1;
    if (!check(!model.applyRows({row("b", "X"), row("b", "Y")}) && model.count() == 3, "duplicate keys corrupted the model")) return 1;
    if (!check(!model.applyRows({row("", "no id")}) && kept.isValid() && model.count() == 3, "empty key corrupted the model")) return 1;
    if (!model.applyRows({})) return 1;
    return check(!kept.isValid() && model.count() == 0 && resets == 0, "clear did not remove rows incrementally") ? 0 : 1;
}
