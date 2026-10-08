#pragma once
#include <QAbstractListModel>
#include <QHash>
#include <QStringList>
#include <QVariantMap>

// The only C++ policy here is Qt's model notification protocol. Discovery,
// ordering, device operations and state reduction belong to the Rust core.
class RecordModel final : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)
public:
    explicit RecordModel(QStringList fields, QObject *parent = nullptr);
    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;
    int count() const { return m_rows.size(); }
    bool apply(const QString &json);
    Q_INVOKABLE QVariantMap get(int index) const;
signals:
    void countChanged();
private:
    QList<QVariantMap> m_rows;
    QHash<int, QByteArray> m_roles;
};

class ServiceModels : public QObject {
    Q_OBJECT
    Q_PROPERTY(QAbstractItemModel* accessPoints READ accessPoints CONSTANT)
    Q_PROPERTY(QAbstractItemModel* bluetoothAdapters READ bluetoothAdapters CONSTANT)
    Q_PROPERTY(QAbstractItemModel* bluetoothDevices READ bluetoothDevices CONSTANT)
    Q_PROPERTY(QAbstractItemModel* backlights READ backlights CONSTANT)
    Q_PROPERTY(QAbstractItemModel* audioNodes READ audioNodes CONSTANT)
    Q_PROPERTY(QAbstractItemModel* mediaPlayers READ mediaPlayers CONSTANT)
    Q_PROPERTY(QAbstractItemModel* niriWindows READ niriWindows CONSTANT)
    Q_PROPERTY(QAbstractItemModel* niriWorkspaces READ niriWorkspaces CONSTANT)
public:
    explicit ServiceModels(QObject *parent = nullptr);
    QAbstractItemModel *accessPoints() const { return m_models.value("accessPoints"); }
    QAbstractItemModel *bluetoothAdapters() const { return m_models.value("bluetoothAdapters"); }
    QAbstractItemModel *bluetoothDevices() const { return m_models.value("bluetoothDevices"); }
    QAbstractItemModel *backlights() const { return m_models.value("backlights"); }
    QAbstractItemModel *audioNodes() const { return m_models.value("audioNodes"); }
    QAbstractItemModel *mediaPlayers() const { return m_models.value("mediaPlayers"); }
    QAbstractItemModel *niriWindows() const { return m_models.value("niriWindows"); }
    QAbstractItemModel *niriWorkspaces() const { return m_models.value("niriWorkspaces"); }
    bool applyCollection(const QString &name, const QString &json);
private:
    QHash<QString, RecordModel*> m_models;
};
