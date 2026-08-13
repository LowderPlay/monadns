use crate::config::InterfaceConfig;
use crate::domain_controller::PolicyId;
use crate::route_controller::{RouteController, sweepable};
use async_trait::async_trait;
use futures::stream::TryStreamExt;
use ipnet::IpNet;
use log::error;
use nftables::batch::Batch;
use nftables::expr::{
    Expression, Meta, MetaKey, NamedExpression, Payload, PayloadField, Prefix, TcpOption,
};
use nftables::schema::*;
use nftables::stmt::{Mangle, Match, NAT, Operator, Statement};
use nftables::types::*;
use nftables::{expr, stmt};
use rtnetlink::packet_route::rule::{RuleAction, RuleAttribute};
use rtnetlink::{IpVersion, new_connection};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

const MAP_V4: &str = "fake_to_real_v4";
const MAP_V6: &str = "fake_to_real_v6";
const MAP_V4_POLICY: &str = "fake_to_policy_v4";
const MAP_V6_POLICY: &str = "fake_to_policy_v6";
const MAP_SUBNET_V4_POLICY: &str = "subnet_to_policy_v4";
const MAP_SUBNET_V6_POLICY: &str = "subnet_to_policy_v6";
const MAP_POLICY_MARK: &str = "policy_to_mark";

#[derive(Clone)]
pub struct NetworkManager {
    interfaces: Arc<RwLock<Vec<InterfaceConfig>>>,
    policy_marks: Arc<RwLock<HashMap<u32, u32>>>,
    nft_table_name: String,
    nft_update_lock: Arc<Mutex<()>>,
}

impl NetworkManager {
    pub fn new(interfaces: Vec<InterfaceConfig>) -> Self {
        Self {
            interfaces: Arc::new(RwLock::new(interfaces)),
            policy_marks: Arc::new(RwLock::new(HashMap::new())),
            nft_table_name: "monadns_steering".to_string(),
            nft_update_lock: Arc::new(Mutex::new(())),
        }
    }

    fn interfaces(&self) -> Vec<InterfaceConfig> {
        self.interfaces.read().unwrap().clone()
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        self.cleanup().await?;
        self.init_routing().await?;
        self.init_nftables()?;

        Ok(())
    }

    async fn init_routing(&self) -> anyhow::Result<()> {
        Self::add_routing_rules_for(&self.interfaces()).await
    }

    async fn add_routing_rules_for(interfaces: &[InterfaceConfig]) -> anyhow::Result<()> {
        let (conn, handle, _) = new_connection()?;
        tokio::spawn(conn);

        for iface in interfaces {
            // Add Rule: fwmark -> table
            handle
                .rule()
                .add()
                .v4()
                .table_id(iface.table_id as u32)
                .fw_mark(iface.fwmark)
                .priority(100)
                .action(RuleAction::ToTable)
                .execute()
                .await?;

            handle
                .rule()
                .add()
                .v6()
                .table_id(iface.table_id as u32)
                .fw_mark(iface.fwmark)
                .priority(100)
                .action(RuleAction::ToTable)
                .execute()
                .await?;
        }

        Ok(())
    }

    async fn cleanup_routing_for(interfaces: &[InterfaceConfig]) -> anyhow::Result<()> {
        let (conn, handle, _) = new_connection()?;
        tokio::spawn(conn);

        for version in [IpVersion::V4, IpVersion::V6] {
            let mut rules = handle.rule().get(version).execute();
            while let Some(rule) = rules.try_next().await? {
                if let Some(mark) = rule.attributes.iter().find_map(|attr| match attr {
                    RuleAttribute::FwMark(m) => Some(*m),
                    _ => None,
                }) {
                    if interfaces
                        .iter()
                        .any(|iface| iface.fwmark == mark && iface.table_id == rule.header.table)
                    {
                        handle.rule().del(rule).execute().await?;
                    }
                }
            }
        }

        Ok(())
    }

    fn init_nftables(&self) -> anyhow::Result<()> {
        let mut batch = Batch::new();
        let family = NfFamily::INet;
        let interfaces = self.interfaces();

        batch.add(NfListObject::Table(Table {
            family,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        }));

        // Flush just in case
        batch.add_cmd(NfCmd::Flush(FlushObject::Table(Table {
            family,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        })));

        let maps = [
            (
                MAP_V4,
                SetTypeValue::Single(SetType::Ipv4Addr),
                SetTypeValue::Single(SetType::Ipv4Addr),
                None,
            ),
            (
                MAP_V6,
                SetTypeValue::Single(SetType::Ipv6Addr),
                SetTypeValue::Single(SetType::Ipv6Addr),
                None,
            ),
            (
                MAP_V4_POLICY,
                SetTypeValue::Single(SetType::Ipv4Addr),
                SetTypeValue::Single(SetType::Mark),
                None,
            ),
            (
                MAP_V6_POLICY,
                SetTypeValue::Single(SetType::Ipv6Addr),
                SetTypeValue::Single(SetType::Mark),
                None,
            ),
            (
                MAP_SUBNET_V4_POLICY,
                SetTypeValue::Single(SetType::Ipv4Addr),
                SetTypeValue::Single(SetType::Mark),
                Some(HashSet::from([SetFlag::Interval])),
            ),
            (
                MAP_SUBNET_V6_POLICY,
                SetTypeValue::Single(SetType::Ipv6Addr),
                SetTypeValue::Single(SetType::Mark),
                Some(HashSet::from([SetFlag::Interval])),
            ),
            (
                MAP_POLICY_MARK,
                SetTypeValue::Single(SetType::Mark),
                SetTypeValue::Single(SetType::Mark),
                None,
            ),
        ];

        for (name, key, value, flags) in maps {
            batch.add(NfListObject::Map(
                Map {
                    family,
                    table: self.nft_table_name.clone().into(),
                    name: name.into(),
                    set_type: key,
                    map: value,
                    flags,
                    ..Default::default()
                }
                .into(),
            ));
        }

        let counters = ["cnt_v4_tx", "cnt_v4_rx", "cnt_v6_tx", "cnt_v6_rx"];
        for name in counters {
            for iface in &interfaces {
                batch.add(NfListObject::Counter(Counter {
                    family,
                    table: self.nft_table_name.clone().into(),
                    name: format!("{}_{}", name, iface.name).into(),
                    ..Default::default()
                }));
            }
        }

        batch.add(NfListObject::Counter(Counter {
            family,
            table: self.nft_table_name.clone().into(),
            name: "cnt_steering_subnet".into(),
            ..Default::default()
        }));
        batch.add(NfListObject::Counter(Counter {
            family,
            table: self.nft_table_name.clone().into(),
            name: "cnt_steering_fakeip".into(),
            ..Default::default()
        }));

        self.add_chains(&mut batch, family);

        self.add_dynamic_rules(&mut batch, family, &interfaces);

        nftables::helper::apply_ruleset(&batch.to_nftables())?;

        Ok(())
    }

    fn add_chains(&self, batch: &mut Batch, family: NfFamily) {
        let chains = [
            (
                Some(NfChainType::Filter),
                Some(NfHook::Prerouting),
                "mangle_prerouting",
                -150,
                None,
            ),
            (
                Some(NfChainType::Route),
                Some(NfHook::Output),
                "mangle_output",
                -150,
                None,
            ),
            (
                Some(NfChainType::NAT),
                Some(NfHook::Prerouting),
                "prerouting",
                -100,
                None,
            ),
            (
                Some(NfChainType::NAT),
                Some(NfHook::Output),
                "output",
                -100,
                None,
            ),
            (
                Some(NfChainType::NAT),
                Some(NfHook::Postrouting),
                "postrouting",
                100,
                None,
            ),
            (
                Some(NfChainType::Filter),
                Some(NfHook::Forward),
                "forward",
                100,
                Some(NfChainPolicy::Accept),
            ),
        ];

        for (ctype, hook, name, prio, policy) in chains {
            batch.add(NfListObject::Chain(Chain {
                _type: ctype,
                family,
                table: self.nft_table_name.clone().into(),
                name: name.into(),
                hook,
                prio: Some(prio),
                policy,
                ..Default::default()
            }));
        }
    }

    fn chain_names() -> [&'static str; 6] {
        [
            "mangle_prerouting",
            "mangle_output",
            "prerouting",
            "output",
            "postrouting",
            "forward",
        ]
    }

    fn ensure_counter(&self, family: NfFamily, name: String) {
        let mut batch = Batch::new();
        batch.add_cmd(NfCmd::Create(NfListObject::Counter(Counter {
            family,
            table: self.nft_table_name.clone().into(),
            name: name.into(),
            ..Default::default()
        })));
        let _ = nftables::helper::apply_ruleset(&batch.to_nftables());
    }

    fn ensure_counters(&self, family: NfFamily, interfaces: &[InterfaceConfig]) {
        for name in ["cnt_steering_subnet", "cnt_steering_fakeip"] {
            self.ensure_counter(family, name.to_string());
        }

        for counter_base in ["cnt_v4_tx", "cnt_v4_rx", "cnt_v6_tx", "cnt_v6_rx"] {
            for iface in interfaces {
                self.ensure_counter(family, format!("{}_{}", counter_base, iface.name));
            }
        }
    }

    fn add_dynamic_rules<'a>(
        &'a self,
        batch: &mut Batch<'a>,
        family: NfFamily,
        interfaces: &'a [InterfaceConfig],
    ) {
        // MTU clamping to avoid fragmentation issues on tunnels
        for iface in interfaces {
            if let Some(mss) = iface.tcp_mss_clamp {
                batch.add(NfListObject::Rule(self.get_mtu_clamp_rule(
                    "forward",
                    iface.fwmark,
                    mss,
                )));
                batch.add(NfListObject::Rule(self.get_mtu_clamp_rule(
                    "output",
                    iface.fwmark,
                    mss,
                )));
            }
        }

        for iface in interfaces {
            batch.add(NfListObject::Rule(
                self.get_accept_incoming_rule(&iface.name),
            ));
        }

        for ip in [IpVersion::V4, IpVersion::V6] {
            let (protocol, subnet_map) = match ip {
                IpVersion::V4 => ("ip", MAP_SUBNET_V4_POLICY),
                IpVersion::V6 => ("ip6", MAP_SUBNET_V6_POLICY),
            };
            batch.add(NfListObject::Rule(self.get_steering_rule(
                "mangle_prerouting",
                protocol,
                subnet_map,
                "cnt_steering_subnet",
            )));
            batch.add(NfListObject::Rule(self.get_steering_rule(
                "mangle_output",
                protocol,
                subnet_map,
                "cnt_steering_subnet",
            )));

            let (protocol, map_name) = match ip {
                IpVersion::V4 => ("ip", MAP_V4_POLICY),
                IpVersion::V6 => ("ip6", MAP_V6_POLICY),
            };
            batch.add(NfListObject::Rule(self.get_steering_rule(
                "mangle_prerouting",
                protocol,
                map_name,
                "cnt_steering_fakeip",
            )));
            batch.add(NfListObject::Rule(self.get_steering_rule(
                "mangle_output",
                protocol,
                map_name,
                "cnt_steering_fakeip",
            )));

            for iface in interfaces {
                batch.add(NfListObject::Rule(self.get_tx_interface_metrics_rule(
                    "mangle_prerouting",
                    ip.clone(),
                    iface,
                )));
                batch.add(NfListObject::Rule(self.get_tx_interface_metrics_rule(
                    "mangle_output",
                    ip.clone(),
                    iface,
                )));
            }

            for iface in interfaces {
                batch.add(NfListObject::Rule(self.get_rx_interface_metrics_rule(
                    "mangle_prerouting",
                    ip.clone(),
                    iface,
                )));
            }

            batch.add(NfListObject::Rule(
                self.get_dnat_rule("prerouting", ip.clone()),
            ));
            batch.add(NfListObject::Rule(self.get_dnat_rule("output", ip.clone())));
        }

        self.add_postrouting_rules(batch, family, interfaces);
    }

    fn rebuild_dynamic_rules(&self, interfaces: &[InterfaceConfig]) -> anyhow::Result<()> {
        let family = NfFamily::INet;
        self.ensure_counters(family, interfaces);

        let mut batch = Batch::new();
        for chain in Self::chain_names() {
            batch.add_cmd(NfCmd::Flush(FlushObject::Chain(Chain {
                family,
                table: self.nft_table_name.clone().into(),
                name: chain.into(),
                ..Default::default()
            })));
        }
        self.add_dynamic_rules(&mut batch, family, interfaces);
        nftables::helper::apply_ruleset(&batch.to_nftables())?;
        Ok(())
    }

    fn match_nfproto(nfproto: &str) -> Statement<'_> {
        Statement::Match(Match {
            left: Expression::Named(NamedExpression::Meta(Meta {
                key: MetaKey::Nfproto,
            })),
            right: Expression::String(nfproto.into()),
            op: Operator::EQ,
        })
    }

    fn add_postrouting_rules<'a>(
        &'a self,
        batch: &mut Batch<'a>,
        family: NfFamily,
        interfaces: &'a [InterfaceConfig],
    ) {
        for iface in interfaces {
            let fwmark_match = Statement::Match(Match {
                left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                right: Expression::Number(iface.fwmark),
                op: Operator::EQ,
            });
            let iface_match = Statement::Match(Match {
                left: Expression::Named(NamedExpression::Meta(Meta {
                    key: MetaKey::Oifname,
                })),
                right: Expression::String(iface.name.clone().into()),
                op: Operator::EQ,
            });

            let stacks = [
                (iface.ipv4_snat, "ip", "ipv4"),
                (iface.ipv6_snat, "ip6", "ipv6"),
            ];

            for (snat, protocol, nfproto) in stacks {
                let mut rules = vec![
                    Self::match_nfproto(nfproto),
                    fwmark_match.clone(),
                    iface_match.clone(),
                ];
                if let Some(snat) = snat {
                    rules.extend(vec![
                        Statement::Match(Match {
                            left: Expression::Named(NamedExpression::Payload(
                                Payload::PayloadField(PayloadField {
                                    protocol: protocol.into(),
                                    field: "saddr".into(),
                                }),
                            )),
                            op: Operator::NEQ,
                            right: Expression::String(snat.to_string().into()),
                        }),
                        Statement::SNAT(Some(NAT {
                            addr: Some(Expression::String(snat.to_string().into())),
                            family: None,
                            port: None,
                            flags: None,
                        })),
                    ]);
                } else {
                    rules.push(Statement::Masquerade(None));
                }

                batch.add(NfListObject::Rule(Rule {
                    family,
                    table: self.nft_table_name.clone().into(),
                    chain: "postrouting".into(),
                    expr: rules.into(),
                    ..Default::default()
                }));
            }
        }
    }

    fn get_mtu_clamp_rule(&self, chain: &'static str, fwmark: u32, max_mss: u32) -> Rule<'_> {
        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    right: Expression::Number(fwmark),
                    op: Operator::EQ,
                }),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Payload(Payload::PayloadField(
                        PayloadField {
                            protocol: "tcp".into(),
                            field: "flags".into(),
                        },
                    ))),
                    op: Operator::EQ,
                    right: Expression::String("syn".into()),
                }),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::TcpOption(TcpOption {
                        name: "maxseg".into(),
                        field: Some("size".into()),
                    })),
                    right: Expression::Number(max_mss),
                    op: Operator::GT,
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::TcpOption(TcpOption {
                        name: "maxseg".into(),
                        field: Some("size".into()),
                    })),
                    value: Expression::Number(max_mss),
                }),
            ]
            .into(),
            ..Default::default()
        }
    }

    fn dest_match_statement<'a>(protocol: &'a str, map_name: &'a str) -> Statement<'a> {
        Statement::Match(Match {
            left: Expression::Named(NamedExpression::Payload(Payload::PayloadField(
                PayloadField {
                    protocol: protocol.into(),
                    field: "daddr".into(),
                },
            ))),
            right: Expression::String(format!("@{}", map_name).into()),
            op: Operator::EQ,
        })
    }

    fn map_expression<'a>(protocol: &'a str, map_name: &'a str) -> Expression<'a> {
        Expression::Named(NamedExpression::Map(Box::new(expr::Map {
            key: Expression::Named(NamedExpression::Payload(Payload::PayloadField(
                PayloadField {
                    protocol: protocol.into(),
                    field: "daddr".into(),
                },
            ))),
            data: Expression::String(format!("@{}", map_name).into()),
        })))
    }

    fn mark_map_expression() -> Expression<'static> {
        Expression::Named(NamedExpression::Map(Box::new(expr::Map {
            key: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
            data: Expression::String(format!("@{}", MAP_POLICY_MARK).into()),
        })))
    }

    fn get_accept_incoming_rule<'a>(&self, if_name: &'a str) -> Rule<'a> {
        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: "mangle_prerouting".into(),
            expr: vec![
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta {
                        key: MetaKey::Iifname,
                    })),
                    right: Expression::String(if_name.into()),
                    op: Operator::EQ,
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    value: Expression::Number(0),
                }),
                Statement::Accept(None),
            ]
            .into(),
            ..Default::default()
        }
    }

    fn get_steering_rule(
        &self,
        chain: &'static str,
        protocol: &'static str,
        policy_map_name: &'static str,
        counter_name: &'static str,
    ) -> Rule<'_> {
        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    right: Expression::Number(0),
                    op: Operator::EQ,
                }),
                Self::dest_match_statement(protocol, policy_map_name),
                Statement::Counter(stmt::Counter::Named(counter_name.into())),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    value: Self::map_expression(protocol, policy_map_name),
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    value: Self::mark_map_expression(),
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "mark".into(),
                        ..Default::default()
                    })),
                    value: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                }),
            ]
            .into(),
            ..Default::default()
        }
    }

    fn get_tx_interface_metrics_rule(
        &self,
        chain: &'static str,
        version: IpVersion,
        iface: &InterfaceConfig,
    ) -> Rule<'_> {
        let counter_base = match version {
            IpVersion::V4 => "cnt_v4_tx",
            IpVersion::V6 => "cnt_v6_tx",
        };
        let nfproto = match version {
            IpVersion::V4 => "ipv4",
            IpVersion::V6 => "ipv6",
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::match_nfproto(nfproto),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    right: Expression::Number(iface.fwmark),
                    op: Operator::EQ,
                }),
                Statement::Counter(stmt::Counter::Named(
                    format!("{}_{}", counter_base, iface.name).into(),
                )),
            ]
            .into(),
            ..Default::default()
        }
    }

    fn get_rx_interface_metrics_rule(
        &self,
        chain: &'static str,
        version: IpVersion,
        iface: &InterfaceConfig,
    ) -> Rule<'_> {
        let counter_base = match version {
            IpVersion::V4 => "cnt_v4_rx",
            IpVersion::V6 => "cnt_v6_rx",
        };
        let nfproto = match version {
            IpVersion::V4 => "ipv4",
            IpVersion::V6 => "ipv6",
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::match_nfproto(nfproto),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    right: Expression::Number(iface.fwmark),
                    op: Operator::EQ,
                }),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "direction".into(),
                        ..Default::default()
                    })),
                    right: Expression::String("reply".into()),
                    op: Operator::EQ,
                }),
                Statement::Counter(stmt::Counter::Named(
                    format!("{}_{}", counter_base, iface.name).into(),
                )),
            ]
            .into(),
            ..Default::default()
        }
    }

    fn get_dnat_rule(&self, chain: &'static str, version: IpVersion) -> Rule<'_> {
        let (protocol, map_name) = match version {
            IpVersion::V4 => ("ip", MAP_V4),
            IpVersion::V6 => ("ip6", MAP_V6),
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::dest_match_statement(protocol, map_name),
                Statement::DNAT(Some(NAT {
                    addr: Some(Self::map_expression(protocol, map_name)),
                    family: None,
                    port: None,
                    flags: None,
                })),
            ]
            .into(),
            ..Default::default()
        }
    }
}

#[async_trait]
impl RouteController for NetworkManager {
    async fn add_mapping(
        &self,
        fake_ip: IpAddr,
        real_ip: IpAddr,
        policy_id: PolicyId,
        replace_existing: bool,
    ) -> anyhow::Result<()> {
        let _guard = self.nft_update_lock.lock().await;
        let policy_key = policy_id.nft_key()?;

        let (map_name, policy_map_name) = match fake_ip {
            IpAddr::V4(_) => (MAP_V4, MAP_V4_POLICY),
            IpAddr::V6(_) => (MAP_V6, MAP_V6_POLICY),
        };

        let mut batch = Batch::new();
        if replace_existing {
            // Delete and replacement are committed as one nftables transaction, so
            // packets can never observe a half-replaced mapping.
            for m in [map_name, policy_map_name] {
                batch.delete(NfListObject::Element(Element {
                    family: NfFamily::INet,
                    table: self.nft_table_name.clone().into(),
                    name: m.into(),
                    elem: vec![Expression::String(fake_ip.to_string().into())].into(),
                }));
            }
        }

        // Add to fake_to_real map
        batch.add(NfListObject::Element(Element {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            name: map_name.into(),
            elem: vec![Expression::List(vec![
                Expression::String(fake_ip.to_string().into()),
                Expression::String(real_ip.to_string().into()),
            ])]
            .into(),
        }));

        // Add to fake_to_policy map
        batch.add(NfListObject::Element(Element {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            name: policy_map_name.into(),
            elem: vec![Expression::List(vec![
                Expression::String(fake_ip.to_string().into()),
                Expression::Number(policy_key),
            ])]
            .into(),
        }));

        nftables::helper::apply_ruleset(&batch.to_nftables())?;
        metrics::counter!("mapped_ip_count", "family" => if real_ip.is_ipv4() { "ipv4" } else { "ipv6" }).increment(1);
        Ok(())
    }

    async fn set_policy_mark(&self, policy_id: PolicyId, fwmark: u32) -> anyhow::Result<()> {
        let _guard = self.nft_update_lock.lock().await;
        let policy_key = policy_id.nft_key()?;

        let mut batch = Batch::new();
        if self.policy_marks.read().unwrap().contains_key(&policy_key) {
            batch.delete(NfListObject::Element(Element {
                family: NfFamily::INet,
                table: self.nft_table_name.clone().into(),
                name: MAP_POLICY_MARK.into(),
                elem: vec![Expression::Number(policy_key)].into(),
            }));
        }
        batch.add(NfListObject::Element(Element {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            name: MAP_POLICY_MARK.into(),
            elem: vec![Expression::List(vec![
                Expression::Number(policy_key),
                Expression::Number(fwmark),
            ])]
            .into(),
        }));

        nftables::helper::apply_ruleset(&batch.to_nftables())?;
        self.policy_marks
            .write()
            .unwrap()
            .insert(policy_key, fwmark);
        Ok(())
    }

    async fn update_interfaces(&self, interfaces: Vec<InterfaceConfig>) -> anyhow::Result<()> {
        let _guard = self.nft_update_lock.lock().await;
        let old_interfaces = self.interfaces();
        let mut routing_cleanup = old_interfaces.clone();
        routing_cleanup.extend(interfaces.clone());

        Self::cleanup_routing_for(&routing_cleanup).await?;
        Self::add_routing_rules_for(&interfaces).await?;
        self.rebuild_dynamic_rules(&interfaces)?;

        *self.interfaces.write().unwrap() = interfaces;
        Ok(())
    }

    async fn cleanup(&self) -> anyhow::Result<()> {
        let _guard = self.nft_update_lock.lock().await;
        let interfaces = self.interfaces();
        let mut batch = Batch::new();

        batch.add_cmd(NfCmd::Delete(NfListObject::Table(Table {
            family: NfFamily::INet,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        })));

        let _ = nftables::helper::apply_ruleset(&batch.to_nftables());

        Self::cleanup_routing_for(&interfaces).await
    }
    async fn fetch_metrics(&self) -> anyhow::Result<()> {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct NftOutput {
            nftables: Vec<HashMap<String, serde_json::Value>>,
        }

        #[derive(Deserialize)]
        struct CounterData {
            table: String,
            name: String,
            packets: u64,
            bytes: u64,
        }

        let raw_json = nftables::helper::get_current_ruleset_raw(
            nftables::helper::DEFAULT_NFT,
            &["list", "counters"],
        )?;

        let output: NftOutput = serde_json::from_str(&raw_json)?;

        for obj in output.nftables {
            if let Some(counter_val) = obj.get("counter") {
                let c: CounterData = serde_json::from_value(counter_val.clone())?;
                if c.table == self.nft_table_name {
                    if c.name == "cnt_steering_subnet" {
                        metrics::gauge!("steering_hits", "type" => "subnet").set(c.packets as f64);
                        metrics::gauge!("steering_bytes", "type" => "subnet").set(c.bytes as f64);
                        continue;
                    }
                    if c.name == "cnt_steering_fakeip" {
                        metrics::gauge!("steering_hits", "type" => "fakeip").set(c.packets as f64);
                        metrics::gauge!("steering_bytes", "type" => "fakeip").set(c.bytes as f64);
                        continue;
                    }

                    let (family, direction, interface) = if c.name.starts_with("cnt_v4_tx") {
                        (
                            "ipv4",
                            "tx",
                            if c.name.len() > 9 {
                                &c.name[10..]
                            } else {
                                "total"
                            },
                        )
                    } else if c.name.starts_with("cnt_v4_rx") {
                        (
                            "ipv4",
                            "rx",
                            if c.name.len() > 9 {
                                &c.name[10..]
                            } else {
                                "total"
                            },
                        )
                    } else if c.name.starts_with("cnt_v6_tx") {
                        (
                            "ipv6",
                            "tx",
                            if c.name.len() > 9 {
                                &c.name[10..]
                            } else {
                                "total"
                            },
                        )
                    } else if c.name.starts_with("cnt_v6_rx") {
                        (
                            "ipv6",
                            "rx",
                            if c.name.len() > 9 {
                                &c.name[10..]
                            } else {
                                "total"
                            },
                        )
                    } else {
                        continue;
                    };

                    metrics::gauge!("intercepted_packets", "family" => family, "direction" => direction, "interface" => interface.to_string()).set(c.packets as f64);
                    metrics::gauge!("intercepted_bytes", "family" => family, "direction" => direction, "interface" => interface.to_string()).set(c.bytes as f64);
                }
            }
        }
        Ok(())
    }

    async fn sync_subnets(&self, subnets: Vec<(String, u32, i64, PolicyId)>) -> anyhow::Result<()> {
        let _guard = self.nft_update_lock.lock().await;
        let mut v4_raw = Vec::new();
        let mut v6_raw = Vec::new();
        let mut policy_marks = HashMap::new();

        for (subnet_str, fwmark, priority, policy_id) in subnets {
            let policy_key = match policy_id.nft_key() {
                Ok(policy_key) => policy_key,
                Err(e) => {
                    error!("Failed to encode policy id '{}': {}", policy_id, e);
                    continue;
                }
            };
            policy_marks.insert(policy_key, fwmark);

            let net: IpNet = match subnet_str.parse() {
                Ok(n) => n,
                Err(_) => match subnet_str.parse::<IpAddr>() {
                    Ok(ip) => IpNet::new(ip, if ip.is_ipv4() { 32 } else { 128 })?,
                    Err(e) => {
                        error!("Failed to parse subnet '{}': {}", subnet_str, e);
                        continue;
                    }
                },
            };

            match net {
                IpNet::V4(v4) => v4_raw.push((v4, policy_key, priority)),
                IpNet::V6(v6) => v6_raw.push((v6, policy_key, priority)),
            }
        }

        let v4_resolved = sweepable::resolve_subnets::<u32>(v4_raw);
        let v6_resolved = sweepable::resolve_subnets::<u128>(v6_raw);

        let mut batch = Batch::new();

        // Flush both maps first
        for map_name in [MAP_SUBNET_V4_POLICY, MAP_SUBNET_V6_POLICY] {
            batch.add_cmd(NfCmd::Flush(FlushObject::Map(Box::new(Map {
                family: NfFamily::INet,
                table: self.nft_table_name.clone().into(),
                name: map_name.into(),
                ..Default::default()
            }))));
        }

        for (resolved, map) in [
            (v4_resolved, MAP_SUBNET_V4_POLICY),
            (v6_resolved, MAP_SUBNET_V6_POLICY),
        ] {
            if !resolved.is_empty() {
                let mut elements = Vec::new();
                for (net, policy_key) in resolved {
                    let prefix = Expression::Named(NamedExpression::Prefix(Prefix {
                        addr: Box::new(Expression::String(net.network().to_string().into())),
                        len: net.prefix_len() as u32,
                    }));
                    elements.push(Expression::List(vec![
                        prefix,
                        Expression::Number(policy_key),
                    ]));
                }

                for chunk in elements.chunks(1000) {
                    batch.add(NfListObject::Element(Element {
                        family: NfFamily::INet,
                        table: self.nft_table_name.clone().into(),
                        name: map.into(),
                        elem: chunk.to_vec().into(),
                    }));
                }
            }
        }

        let installed_policy_marks = self.policy_marks.read().unwrap().clone();
        for policy_key in policy_marks.keys() {
            if installed_policy_marks.contains_key(policy_key) {
                batch.delete(NfListObject::Element(Element {
                    family: NfFamily::INet,
                    table: self.nft_table_name.clone().into(),
                    name: MAP_POLICY_MARK.into(),
                    elem: vec![Expression::Number(*policy_key)].into(),
                }));
            }
        }

        for (policy_key, fwmark) in &policy_marks {
            batch.add(NfListObject::Element(Element {
                family: NfFamily::INet,
                table: self.nft_table_name.clone().into(),
                name: MAP_POLICY_MARK.into(),
                elem: vec![Expression::List(vec![
                    Expression::Number(*policy_key),
                    Expression::Number(*fwmark),
                ])]
                .into(),
            }));
        }

        nftables::helper::apply_ruleset(&batch.to_nftables())?;
        self.policy_marks.write().unwrap().extend(policy_marks);
        Ok(())
    }
}
