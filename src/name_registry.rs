use enumflags2::BitFlags;
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::debug;
use zbus::{
    fdo::{ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::{BusName, OwnedBusName, OwnedUniqueName, OwnedWellKnownName, WellKnownName},
};

#[derive(Clone, Debug)]
pub struct NameRegistry {
    names: HashMap<OwnedWellKnownName, NameEntry>,
    name_changed_tx: Sender<NameOwnerChanged>,
}

#[derive(Clone, Debug)]
pub struct NameEntry {
    owner: NameOwner,
    waiting_list: VecDeque<NameOwner>,
}

impl NameEntry {
    pub fn owner(&self) -> &NameOwner {
        &self.owner
    }

    pub fn waiting_list(&self) -> impl Iterator<Item = &NameOwner> {
        self.waiting_list.iter()
    }
}

#[derive(Clone, Debug)]
pub struct NameOwner {
    unique_name: OwnedUniqueName,
    allow_replacement: bool,
}

impl NameOwner {
    pub fn unique_name(&self) -> &OwnedUniqueName {
        &self.unique_name
    }
}

impl NameRegistry {
    pub fn new() -> (Self, Receiver<NameOwnerChanged>) {
        let (name_changed_tx, name_changed_rx) = tokio::sync::mpsc::channel(64);
        (
            Self {
                names: HashMap::new(),
                name_changed_tx,
            },
            name_changed_rx,
        )
    }

    pub async fn request_name(
        &mut self,
        name: OwnedWellKnownName,
        unique_name: OwnedUniqueName,
        flags: BitFlags<RequestNameFlags>,
    ) -> RequestNameReply {
        let owner = NameOwner {
            unique_name: unique_name.clone(),
            allow_replacement: flags.contains(RequestNameFlags::AllowReplacement),
        };

        match self.names.get_mut(&name) {
            Some(entry) => {
                if entry.owner.unique_name == unique_name {
                    RequestNameReply::AlreadyOwner
                } else if flags.contains(RequestNameFlags::ReplaceExisting)
                    && entry.owner.allow_replacement
                {
                    let old_owner = entry.owner.unique_name.clone();
                    entry.owner = owner;

                    if let Err(e) = self
                        .name_changed_tx
                        .send(NameOwnerChanged {
                            name: BusName::from(name).into(),
                            old_owner: Some(old_owner),
                            new_owner: Some(unique_name),
                        })
                        .await
                    {
                        debug!("failed to send NameOwnerChanged: {e}");
                    }
                    RequestNameReply::PrimaryOwner
                } else if !flags.contains(RequestNameFlags::DoNotQueue) {
                    entry.waiting_list.push_back(owner);

                    RequestNameReply::InQueue
                } else {
                    RequestNameReply::Exists
                }
            }
            None => {
                self.names.insert(
                    name.clone(),
                    NameEntry {
                        owner,
                        waiting_list: VecDeque::new(),
                    },
                );

                if let Err(e) = self
                    .name_changed_tx
                    .send(NameOwnerChanged {
                        name: BusName::from(name).into(),
                        old_owner: None,
                        new_owner: Some(unique_name),
                    })
                    .await
                {
                    debug!("failed to send NameOwnerChanged: {e}");
                }

                RequestNameReply::PrimaryOwner
            }
        }
    }

    pub async fn release_name(
        &mut self,
        name: OwnedWellKnownName,
        owner: OwnedUniqueName,
    ) -> ReleaseNameReply {
        let name_changed_tx = self.name_changed_tx.clone();
        match self.names.get_mut(name.as_str()) {
            Some(entry) => {
                if *entry.owner.unique_name == owner {
                    let new_owner_name = match entry.waiting_list.pop_front() {
                        Some(owner) => {
                            entry.owner = owner;
                            Some(entry.owner.unique_name.clone())
                        }
                        None => {
                            self.names.remove(name.as_str());

                            None
                        }
                    };

                    if let Err(e) = name_changed_tx
                        .send(NameOwnerChanged {
                            name: BusName::from(name).into(),
                            old_owner: Some(owner),
                            new_owner: new_owner_name,
                        })
                        .await
                    {
                        debug!("failed to send NameOwnerChanged: {e}");
                    }

                    ReleaseNameReply::Released
                } else {
                    for (i, waiting) in entry.waiting_list.iter().enumerate() {
                        if *waiting.unique_name == owner {
                            entry.waiting_list.remove(i);

                            return ReleaseNameReply::Released;
                        }
                    }

                    ReleaseNameReply::NonExistent
                }
            }
            None => ReleaseNameReply::NonExistent,
        }
    }

    pub async fn release_all(&mut self, owner: OwnedUniqueName) {
        // Find all names registered and queued by the given owner.
        let names: Vec<_> = self
            .names
            .iter()
            .filter_map(|(name, entry)| {
                if *entry.owner.unique_name == owner
                    || entry
                        .waiting_list
                        .iter()
                        .any(|waiting| *waiting.unique_name == owner)
                {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        // Now release our claim or waiting list tickets from all these names.
        for name in names {
            self.release_name(name, owner.clone()).await;
        }
    }

    pub fn lookup(&self, name: WellKnownName) -> Option<OwnedUniqueName> {
        self.names
            .get(name.as_str())
            .map(|e| e.owner.unique_name.clone())
    }

    pub fn all_names(&self) -> &HashMap<OwnedWellKnownName, NameEntry> {
        &self.names
    }

    pub fn waiting_list(
        &self,
        name: WellKnownName<'_>,
    ) -> Option<impl Iterator<Item = &NameOwner>> {
        self.names.get(name.as_str()).map(|e| e.waiting_list.iter())
    }
}

#[derive(Debug)]
pub struct NameOwnerChanged {
    pub name: OwnedBusName,
    pub old_owner: Option<OwnedUniqueName>,
    pub new_owner: Option<OwnedUniqueName>,
}
