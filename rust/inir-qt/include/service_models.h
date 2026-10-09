#pragma once
#include <QAbstractListModel>
#include <QHash>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>

// The only C++ policy here is Qt's model notification protocol. Discovery,
// ordering, device operations and state reduction belong to the Rust core.
// Rows arrive as typed QVariantMaps built in Rust, so no JSON is parsed here.
class RecordModel final : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)
public:
    explicit RecordModel(QStringList fields, QObject *parent = nullptr);
    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;
    int count() const { return m_rows.size(); }
    bool applyRows(const QVariantList &rows);
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
    Q_PROPERTY(QAbstractItemModel* backlights READ backlights CONSTANT)
    Q_PROPERTY(QAbstractItemModel* niriWindows READ niriWindows CONSTANT)
    Q_PROPERTY(QAbstractItemModel* niriWorkspaces READ niriWorkspaces CONSTANT)
public:
    explicit ServiceModels(QObject *parent = nullptr);
    QAbstractItemModel *accessPoints() const { return m_models.value("accessPoints"); }
    QAbstractItemModel *backlights() const { return m_models.value("backlights"); }
    QAbstractItemModel *niriWindows() const { return m_models.value("niriWindows"); }
    QAbstractItemModel *niriWorkspaces() const { return m_models.value("niriWorkspaces"); }
    bool applyCollection(const QString &name, const QVariant &rows);
private:
    QHash<QString, RecordModel*> m_models;
};
