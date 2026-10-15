use cxx_qt_lib::{QList,QMap,QMapPair_QString_QVariant,QString,QVariant};
use inir_types::desktop::IpcValue;
use std::collections::BTreeMap;
type Map = QMap<QMapPair_QString_QVariant>;
pub fn map<const N:usize>(items:[(&str,IpcValue);N]) -> IpcValue {
    IpcValue::Map(items.into_iter().map(|(key,value)|(key.into(),value)).collect())
}
pub fn from_variant(value:&QVariant) -> IpcValue {
    match value.type_name() {
        "bool" => IpcValue::Bool(value.value_or_default::<bool>()),
        "QString" => IpcValue::String(value.value_or_default::<QString>().to_string()),
        "QVariantMap" | "QMap<QString,QVariant>" | "QMap<QString, QVariant>" => {
            let map=value.value_or_default::<Map>();
            IpcValue::Map(map.iter().map(|(k,v)|(k.to_string(),from_variant(v))).collect())
        }
        "QVariantList" | "QList<QVariant>" => IpcValue::List(value.value_or_default::<QList<QVariant>>().iter().map(from_variant).collect()),
        "double" | "float" | "int" | "uint" | "qlonglong" | "qulonglong" => IpcValue::Number(value.value_or_default::<f64>()),
        _ => IpcValue::Null,
    }
}
pub fn variant(value:&IpcValue) -> QVariant {
    match value {
        IpcValue::Null => crate::qobjects::niri::qobject::null_variant(),
        IpcValue::Bool(v) => QVariant::from(v),
        IpcValue::Number(v) => QVariant::from(v),
        IpcValue::String(v) => QVariant::from(&QString::from(v)),
        IpcValue::List(values) => {
            let mut list=QList::<QVariant>::default();
            for value in values { list.append(variant(value)); }
            QVariant::from(&list)
        }
        IpcValue::Map(values) => outputs(values),
    }
}
pub fn outputs(values:&BTreeMap<String,IpcValue>) -> QVariant {
    let mut map=Map::default();
    for (key,value) in values { map.insert(QString::from(key),variant(value)); }
    QVariant::from(&map)
}
pub fn strings(values:&[String]) -> QVariant {
    let mut list=QList::<QVariant>::default();
    for value in values { list.append(QVariant::from(&QString::from(value))); }
    QVariant::from(&list)
}
pub fn numbers(values:&[u64]) -> QVariant {
    let mut list=QList::<QVariant>::default();
    for value in values { list.append(QVariant::from(&(*value as f64))); }
    QVariant::from(&list)
}
/// Active connections as a QVariantList of {id,type,uuid,vpn,state} maps for QML.
pub fn active_connections(values:&[inir_types::desktop::ActiveConnection]) -> QVariant {
    let mut list=QList::<QVariant>::default();
    for value in values {
        let mut map=Map::default();
        map.insert(QString::from("id"), QVariant::from(&QString::from(&value.id)));
        map.insert(QString::from("type"), QVariant::from(&QString::from(&value.kind)));
        map.insert(QString::from("uuid"), QVariant::from(&QString::from(&value.uuid)));
        map.insert(QString::from("vpn"), QVariant::from(&value.vpn));
        map.insert(QString::from("state"), QVariant::from(&(value.state as i32)));
        list.append(QVariant::from(&map));
    }
    QVariant::from(&list)
}
