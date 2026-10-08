#include "inir-qt/service_models.h"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSet>
#include <QThread>

RecordModel::RecordModel(QStringList fields, QObject *parent) : QAbstractListModel(parent) {
    m_roles.insert(Qt::UserRole, "record");
    int role = Qt::UserRole + 1;
    for (const auto &field : fields) m_roles.insert(role++, field.toUtf8());
}
int RecordModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : count(); }
QVariant RecordModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.column() != 0 || index.row() < 0 || index.row() >= count()) return {};
    if (role == Qt::UserRole) return m_rows[index.row()];
    return m_rows[index.row()].value(QString::fromUtf8(m_roles.value(role)));
}
QHash<int, QByteArray> RecordModel::roleNames() const { return m_roles; }
QVariantMap RecordModel::get(int index) const { return index >= 0 && index < count() ? m_rows[index] : QVariantMap{}; }
bool RecordModel::apply(const QString &json) {
    Q_ASSERT(QThread::currentThread() == thread());
    QJsonParseError error;
    const auto document = QJsonDocument::fromJson(json.toUtf8(), &error);
    if (error.error != QJsonParseError::NoError || !document.isArray()) return false;
    QList<QVariantMap> next;
    QSet<QString> ids;
    for (const auto &value : document.array()) {
        if (!value.isObject()) return false;
        const auto row = value.toObject().toVariantMap();
        const auto id = row.value("id").toString();
        if (id.isEmpty() || ids.contains(id)) return false;
        ids.insert(id); next.append(row);
    }
    const int oldCount = count();
    // Remove only absent keys, then insert/move surviving rows into order.
    // Stable device/window IDs preserve delegates and persistent indexes.
    for (int i = count() - 1; i >= 0; --i) {
        if (!ids.contains(m_rows[i].value("id").toString())) {
            beginRemoveRows({}, i, i); m_rows.removeAt(i); endRemoveRows();
        }
    }
    for (int i = 0; i < next.size(); ++i) {
        const auto id = next[i].value("id");
        int found = i;
        while (found < count() && m_rows[found].value("id") != id) ++found;
        if (found == count()) {
            beginInsertRows({}, i, i); m_rows.insert(i, next[i]); endInsertRows();
        } else {
            if (found != i) { beginMoveRows({}, found, found, {}, i); m_rows.move(found, i); endMoveRows(); }
            if (m_rows[i] != next[i]) {
                QList<int> changed;
                changed.append(Qt::UserRole);
                for (auto it = m_roles.cbegin(); it != m_roles.cend(); ++it) {
                    if (it.key() != Qt::UserRole && m_rows[i].value(QString::fromUtf8(it.value())) != next[i].value(QString::fromUtf8(it.value()))) changed.append(it.key());
                }
                m_rows[i] = next[i]; emit dataChanged(index(i), index(i), changed);
            }
        }
    }
    if (oldCount != count()) emit countChanged();
    return true;
}

ServiceModels::ServiceModels(QObject *parent) : QObject(parent) {
    auto add = [this](const QString &name, const QStringList &fields) { m_models.insert(name, new RecordModel(fields, this)); };
    add("accessPoints", {"id", "device", "ssid", "bssid", "strength", "frequency", "rate", "security", "active"});
    add("bluetoothAdapters", {"id", "address", "name", "powered", "discovering"});
    add("bluetoothDevices", {"id", "adapter", "address", "name", "icon", "paired", "trusted", "connected", "battery"});
    add("backlights", {"id", "kind", "raw", "maximum", "value"});
    add("audioNodes", {"id", "serial", "name", "description", "mediaClass", "volume", "muted", "channels"});
    add("mediaPlayers", {"id", "identity", "playbackStatus", "title", "artist", "artUrl", "length", "canControl", "canSeek"});
    add("niriWindows", {"id", "title", "appId", "workspaceId", "focused", "floating", "urgent", "focusSerial"});
    add("niriWorkspaces", {"id", "index", "name", "output", "active", "focused", "activeWindowId", "urgent"});
}
bool ServiceModels::applyCollection(const QString &name, const QString &json) {
    const auto model = m_models.value(name); return model && model->apply(json);
}
