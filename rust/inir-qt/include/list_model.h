#pragma once
#include <QAbstractListModel>
#include <QStringList>
#include <QVariant>
#include <QJSValue>
#include <QJSEngine>
#include <QQmlEngine>
inline QJSValue scriptValue(QObject *owner, const QVariant &value);
// QML object/array arguments can arrive wrapped in QJSValue rather than as a
// QVariantMap/List. Materialize them on the Qt thread before Rust reads fields.
inline QVariant commandVariant(const QVariant &value) {
    return value.metaType().id() == qMetaTypeId<QJSValue>()
        ? value.value<QJSValue>().toVariant() : value;
}

class UtilityJobsBase : public QObject {
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE QJSValue jsValue(const QVariant &value) { return scriptValue(this, value); }
};

// Qt notification trampoline only. No rows, snapshots, diffing or JSON here.
class ListModelBase : public QAbstractListModel {
    Q_OBJECT
public:
    using QAbstractListModel::QAbstractListModel;
    Q_INVOKABLE QJSValue jsValue(const QVariant &value) { return scriptValue(this, value); }
    virtual int rowCountFlat() const = 0;
    virtual QVariant rowData(int row, int role) const = 0;
    int rowCount(const QModelIndex &parent = {}) const override { return parent.isValid() ? 0 : rowCountFlat(); }
    QVariant data(const QModelIndex &index, int role) const override {
        if (!index.isValid() || index.row() < 0 || index.row() >= rowCountFlat()) return {};
        return rowData(index.row(), role);
    }
    QHash<int, QByteArray> roleNames() const override { return m_roles; }
    void setRoles(const QStringList &roles) {
        int role = Qt::UserRole + 1;
        for (const auto &name : roles) m_roles.insert(role++, name.toUtf8());
    }
    void insertStart(int row) { beginInsertRows({}, row, row); }
    void resetStart() { beginResetModel(); }
    void resetEnd() { endResetModel(); }
    void insertEnd() { endInsertRows(); }
    void removeStart(int row) { beginRemoveRows({}, row, row); }
    void removeEnd() { endRemoveRows(); }
    void moveStart(int from, int to) { beginMoveRows({}, from, from, {}, to > from ? to + 1 : to); }
    void moveEnd() { endMoveRows(); }
    void changed(int row, quint64 mask) {
        changedRange(row, row, mask);
    }
    void changedRange(int first, int last, quint64 mask) {
        QList<int> roles;
        for (int bit = 0; bit < 64; ++bit)
            if (mask & (quint64(1) << bit)) roles.append(Qt::UserRole + 1 + bit);
        emit dataChanged(index(first), index(last), roles);
    }
private:
    QHash<int, QByteArray> m_roles;
};
inline QVariant nullVariant() { return QVariant::fromValue(nullptr); }
class NiriListModelBase : public ListModelBase { Q_OBJECT public: using ListModelBase::ListModelBase; };
class WorkspaceListModelBase : public ListModelBase { Q_OBJECT public: using ListModelBase::ListModelBase; };
class NetworkListModelBase : public ListModelBase { Q_OBJECT public: using ListModelBase::ListModelBase; };
class BrightnessListModelBase : public ListModelBase { Q_OBJECT public: using ListModelBase::ListModelBase; };
class BluetoothControlModelBase : public ListModelBase { Q_OBJECT public: using ListModelBase::ListModelBase; };
// QML requires actual arrays for nested geometry/extension data. Only convert
// changed small metadata, not whole window/workspace collections.
inline QJSValue scriptValue(QObject *owner, const QVariant &value) {
    auto *engine = qjsEngine(owner);
    if (!engine) return {};
    if (!value.isValid() || value.isNull()) return QJSValue(QJSValue::NullValue);
    if (value.metaType().id() == QMetaType::QVariantList) {
        const auto list = value.toList();
        auto array = engine->newArray(list.size());
        for (int i = 0; i < list.size(); ++i) array.setProperty(i, scriptValue(owner, list[i]));
        return array;
    }
    if (value.metaType().id() == QMetaType::QVariantMap) {
        const auto map = value.toMap();
        auto object = engine->newObject();
        for (auto it = map.cbegin(); it != map.cend(); ++it) object.setProperty(it.key(), scriptValue(owner, it.value()));
        return object;
    }
    return engine->toScriptValue(value);
}
