use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use super::dictionary::Dictionary;
use super::query::{Route, is_single_token, normalize_submit, route, senses};
use super::{
    CompatiblePreview, Config, DictionaryStatus, InstallProgress, LookupError, LookupErrorKind,
    Outcome, Phase, PhraseResult, Snapshot, Source, Speaking, SpeechRate, WordResult,
};

struct LocalJob {
    generation: u64,
    text: String,
    cancel: Arc<AtomicBool>,
}

struct OnlineJob {
    preview: CompatiblePreview,
    credential: Option<String>,
    deadline: Duration,
    cancel: Arc<AtomicBool>,
}

struct State {
    snapshot: Snapshot,
    config: Config,
    audio_busy: bool,
    local_cancel: Arc<AtomicBool>,
    online_cancel: Arc<AtomicBool>,
    install_cancel: Arc<AtomicBool>,
    install_generation: u64,
    install_pending: bool,
    install_running: bool,
    speech_generation: u64,
    speaking_token: u64,
    local_job: Option<LocalJob>,
    online_job: Option<OnlineJob>,
    recheck: bool,
    dictionary: Option<Arc<Dictionary>>,
    #[allow(dead_code)]
    dictionary_key: Option<String>,
}

struct Engine {
    state: Arc<Mutex<State>>,
    tx: Sender<()>,
    probe_tx: Sender<()>,
    install_tx: Sender<()>,
    wake: Arc<AtomicBool>,
    probe_wake: Arc<AtomicBool>,
    install_wake: Arc<AtomicBool>,
    #[cfg(test)]
    manual: bool,
    #[cfg(test)]
    speech: Arc<Mutex<FakeSpeech>>,
    #[cfg(test)]
    install_hook: Arc<Mutex<Option<InstallHook>>>,
}

#[cfg(test)]
type InstallHook = Arc<dyn Fn(&AtomicBool) -> Result<DictionaryStatus, String> + Send + Sync>;

#[cfg(test)]
struct QueuedSpeech {
    token: u64,
    command: super::speech::SpeechCommand,
}

#[cfg(test)]
struct FakeSpeech {
    fence: super::speech::SpeechFence,
    queue: std::collections::VecDeque<QueuedSpeech>,
    hold_main: bool,
    speaking: bool,
    starts: u64,
    stops: u64,
}

fn service() -> &'static Engine {
    static SERVICE: OnceLock<Engine> = OnceLock::new();
    SERVICE.get_or_init(Engine::live)
}

impl Engine {
    fn live() -> Self {
        let (tx, rx) = mpsc::channel();
        let (probe_tx, probe_rx) = mpsc::channel();
        let (install_tx, install_rx) = mpsc::channel();
        let engine = Self {
            state: Arc::new(Mutex::new(State {
                snapshot: Snapshot::idle(),
                config: Config::default(),
                audio_busy: false,
                local_cancel: Arc::new(AtomicBool::new(false)),
                online_cancel: Arc::new(AtomicBool::new(false)),
                install_cancel: Arc::new(AtomicBool::new(false)),
                install_generation: 0,
                install_pending: false,
                install_running: false,
                speech_generation: 0,
                speaking_token: 0,
                local_job: None,
                online_job: None,
                recheck: false,
                dictionary: None,
                dictionary_key: None,
            })),
            tx,
            probe_tx,
            install_tx,
            wake: Arc::new(AtomicBool::new(false)),
            probe_wake: Arc::new(AtomicBool::new(false)),
            install_wake: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            manual: false,
            #[cfg(test)]
            speech: Arc::new(Mutex::new(FakeSpeech {
                fence: super::speech::SpeechFence::default(),
                queue: std::collections::VecDeque::new(),
                hold_main: false,
                speaking: false,
                starts: 0,
                stops: 0,
            })),
            #[cfg(test)]
            install_hook: Arc::new(Mutex::new(None)),
        };
        let worker = engine.clone_worker();
        let _ = thread::Builder::new()
            .name("hex-translation".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    worker.run_turn();
                }
            });
        let probe = engine.clone_worker();
        let _ = thread::Builder::new()
            .name("hex-translation-probe".into())
            .spawn(move || {
                while probe_rx.recv().is_ok() {
                    probe.run_probe();
                }
            });
        let installer = engine.clone_worker();
        let _ = thread::Builder::new()
            .name("hex-translation-install".into())
            .spawn(move || {
                while install_rx.recv().is_ok() {
                    installer.run_install_turn();
                }
            });
        engine
    }

    fn clone_worker(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            tx: self.tx.clone(),
            probe_tx: self.probe_tx.clone(),
            install_tx: self.install_tx.clone(),
            wake: Arc::clone(&self.wake),
            probe_wake: Arc::clone(&self.probe_wake),
            install_wake: Arc::clone(&self.install_wake),
            #[cfg(test)]
            manual: self.manual,
            #[cfg(test)]
            speech: Arc::clone(&self.speech),
            #[cfg(test)]
            install_hook: Arc::clone(&self.install_hook),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn kick(&self) {
        #[cfg(test)]
        if self.manual {
            self.wake.store(true, Ordering::Release);
            return;
        }
        if self.wake.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.tx.send(());
    }

    fn run_turn(&self) {
        loop {
            self.wake.store(false, Ordering::Release);
            let (local, online) = {
                let mut state = self.lock();
                (state.local_job.take(), state.online_job.take())
            };
            if local.is_none() && online.is_none() {
                if !self.wake.load(Ordering::Acquire) {
                    break;
                }
                continue;
            }
            if let Some(job) = local {
                run_local(self, job);
            }
            if let Some(job) = online {
                run_online(self, job);
            }
            if !self.wake.load(Ordering::Acquire) {
                break;
            }
        }
    }

    fn kick_probe(&self) {
        #[cfg(test)]
        if self.manual {
            self.probe_wake.store(true, Ordering::Release);
            return;
        }
        if self.probe_wake.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.probe_tx.send(());
    }

    fn run_probe(&self) {
        loop {
            self.probe_wake.store(false, Ordering::Release);
            let enabled = {
                let mut state = self.lock();
                state.recheck = false;
                state.snapshot.enabled
            };
            if enabled {
                refresh_resources(self);
            }
            if !self.probe_wake.load(Ordering::Acquire) {
                break;
            }
        }
    }

    fn configure(&self, config: Config) {
        let mut stop = false;
        let mut recheck = false;
        {
            let mut state = self.lock();
            if same_config(&state.config, &config) {
                return;
            }
            let was_enabled = state.config.enabled;
            let compatible_changed = compatible_changed(&state.config, &config);
            state.config = config;
            if !state.config.enabled {
                invalidate(&mut state, true);
                state.snapshot.enabled = false;
                state.snapshot.text.clear();
                state.snapshot.phase = Phase::Idle;
                state.dictionary = None;
                state.recheck = false;
                bump(&mut state);
                stop = true;
            } else {
                state.snapshot.enabled = true;
                if compatible_changed {
                    cancel_online(&mut state);
                    if matches!(
                        state.snapshot.phase,
                        Phase::OnlinePreview | Phase::OnlinePending
                    ) {
                        state.snapshot.phase = Phase::Idle;
                    }
                }
                if !was_enabled {
                    state.recheck = true;
                    recheck = true;
                }
                bump(&mut state);
            }
        }
        if stop {
            self.native_stop();
            return;
        }
        if recheck {
            self.kick_probe();
        }
    }

    fn snapshot(&self) -> Snapshot {
        let (speaking, token) = {
            let state = self.lock();
            (state.snapshot.speaking.clone(), state.speaking_token)
        };
        if speaking.is_some() && !self.native_speaking() {
            let mut state = self.lock();
            if state.speaking_token == token && state.snapshot.speaking == speaking {
                state.snapshot.speaking = None;
                bump(&mut state);
            }
        }
        self.lock().snapshot.clone()
    }

    fn edit(&self, text: String) {
        {
            let mut state = self.lock();
            if !state.snapshot.enabled {
                return;
            }
            invalidate(&mut state, false);
            state.snapshot.text = text;
            state.snapshot.phase = Phase::Idle;
            bump(&mut state);
        }
        self.native_stop();
    }

    fn submit_local(&self) {
        let (halt, queued) = {
            let mut state = self.lock();
            if !state.snapshot.enabled {
                return;
            }
            match normalize_submit(&state.snapshot.text) {
                Err(error) => {
                    invalidate(&mut state, false);
                    state.snapshot.error = Some(error);
                    state.snapshot.phase = Phase::Ready;
                    bump(&mut state);
                    (true, false)
                }
                Ok(text) if already_in_flight(&state, &text) => (false, false),
                Ok(text) => {
                    invalidate(&mut state, false);
                    state.snapshot.text = text.clone();
                    state.snapshot.phase = Phase::LocalPending;
                    let cancel = Arc::clone(&state.local_cancel);
                    let generation = state.snapshot.generation;
                    state.local_job = Some(LocalJob {
                        generation,
                        text,
                        cancel,
                    });
                    bump(&mut state);
                    (true, true)
                }
            }
        };
        if halt {
            self.native_stop();
        }
        if queued {
            self.kick();
        }
    }

    fn cancel(&self) {
        {
            let mut state = self.lock();
            if !state.snapshot.enabled {
                return;
            }
            invalidate(&mut state, false);
            state.snapshot.phase = Phase::Idle;
            bump(&mut state);
        }
        self.native_stop();
    }

    fn prepare_online(&self) -> Result<CompatiblePreview, LookupError> {
        let mut state = self.lock();
        if !state.snapshot.enabled {
            return Err(LookupError::new(LookupErrorKind::Canceled));
        }
        let text = normalize_submit(&state.snapshot.text)?;
        let preview = super::compatible::preview(
            state.snapshot.generation,
            &text,
            &state.config.compatible_base_url,
            &state.config.compatible_model,
        )?;
        cancel_local(&mut state);
        state.snapshot.online_preview = Some(preview.clone());
        state.snapshot.phase = Phase::OnlinePreview;
        bump(&mut state);
        Ok(preview)
    }

    fn confirm_online(&self, preview: CompatiblePreview) {
        let Some(path) = ({
            let state = self.lock();
            if !confirm_binding(&state, &preview) {
                None
            } else {
                Some(state.config.compatible_credential_path.clone())
            }
        }) else {
            return;
        };
        let credential = match super::compatible::capture_credential(&path, &preview.base_url) {
            Ok(credential) => credential,
            Err(error) => {
                let mut state = self.lock();
                if confirm_binding(&state, &preview) {
                    state.snapshot.error = Some(error);
                    bump(&mut state);
                }
                return;
            }
        };
        let send = {
            let mut state = self.lock();
            if !confirm_binding(&state, &preview) || state.config.compatible_credential_path != path
            {
                false
            } else {
                state.snapshot.online_preview = None;
                cancel_local(&mut state);
                cancel_online(&mut state);
                let cancel = Arc::clone(&state.online_cancel);
                state.snapshot.phase = Phase::OnlinePending;
                state.snapshot.error = None;
                state.online_job = Some(OnlineJob {
                    preview,
                    credential,
                    deadline: state.config.compatible_deadline.max(Duration::from_secs(1)),
                    cancel,
                });
                bump(&mut state);
                true
            }
        };
        if send {
            self.kick();
        }
    }

    fn install_dictionary(&self) {
        let admitted = {
            let mut state = self.lock();
            if !state.snapshot.enabled {
                return;
            }
            let canceled = state.install_cancel.load(Ordering::Acquire);
            if (state.install_running || state.install_pending) && !canceled {
                return;
            }
            state.install_cancel.store(true, Ordering::Release);
            let cancel = Arc::new(AtomicBool::new(false));
            state.install_cancel = Arc::clone(&cancel);
            state.install_generation = state.install_generation.saturating_add(1);
            state.install_pending = true;
            let progress = InstallProgress {
                received: 0,
                total: super::dictionary::DOWNLOAD_BYTES,
                stage: super::InstallStage::Dictionary,
            };
            state.snapshot.dictionary_install = Some(progress.clone());
            state.snapshot.resources.dictionary = DictionaryStatus::Installing {
                received: 0,
                total: progress.total,
                stage: progress.stage,
            };
            bump(&mut state);
            true
        };
        if admitted {
            self.kick_install();
        }
    }

    fn cancel_install(&self) {
        let mut state = self.lock();
        state.install_cancel.store(true, Ordering::Release);
        state.install_pending = false;
        if !state.install_running
            && matches!(
                state.snapshot.resources.dictionary,
                DictionaryStatus::Installing { .. }
            )
        {
            state.snapshot.dictionary_install = None;
            state.snapshot.resources.dictionary = super::dictionary::status_on_disk();
            bump(&mut state);
        }
    }

    fn kick_install(&self) {
        #[cfg(test)]
        if self.manual {
            self.install_wake.store(true, Ordering::Release);
            return;
        }
        if self.install_wake.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.install_tx.send(());
    }

    fn run_install_turn(&self) {
        loop {
            self.install_wake.store(false, Ordering::Release);
            let job = {
                let mut state = self.lock();
                if !state.install_pending {
                    None
                } else if !state.snapshot.enabled {
                    state.install_pending = false;
                    None
                } else {
                    state.install_pending = false;
                    state.install_running = true;
                    Some((state.install_generation, Arc::clone(&state.install_cancel)))
                }
            };
            if let Some((generation, cancel)) = job {
                self.run_install_job(generation, cancel);
            }
            if !self.install_wake.load(Ordering::Acquire) {
                break;
            }
        }
    }

    fn run_install_job(&self, generation: u64, cancel: Arc<AtomicBool>) {
        let result = self.do_install(generation, &cancel);
        let mut state = self.lock();
        state.install_running = false;
        if state.install_generation != generation {
            return;
        }
        state.snapshot.dictionary_install = None;
        if !state.snapshot.enabled {
            bump(&mut state);
            return;
        }
        match result {
            Ok(status) if !cancel.load(Ordering::Acquire) => {
                state.snapshot.resources.dictionary = status;
                state.dictionary = None;
                state.dictionary_key = None;
            }
            Err(message) if !cancel.load(Ordering::Acquire) && !is_cancel_message(&message) => {
                state.snapshot.resources.dictionary = DictionaryStatus::Failed { message };
            }
            _ => {
                state.snapshot.resources.dictionary = super::dictionary::status_on_disk();
            }
        }
        bump(&mut state);
    }

    fn do_install(&self, generation: u64, cancel: &AtomicBool) -> Result<DictionaryStatus, String> {
        #[cfg(test)]
        if self.manual {
            let _ = generation;
            let hook = self
                .install_hook
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            return match hook {
                Some(hook) => hook(cancel),
                None => Err("test install is not connected".into()),
            };
        }
        let shared = Arc::clone(&self.state);
        super::dictionary::install(
            |progress| {
                let mut state = shared.lock().unwrap_or_else(|error| error.into_inner());
                if state.install_generation != generation
                    || cancel.load(Ordering::Acquire)
                    || !state.snapshot.enabled
                {
                    return;
                }
                state.snapshot.dictionary_install = Some(progress.clone());
                state.snapshot.resources.dictionary = DictionaryStatus::Installing {
                    received: progress.received,
                    total: progress.total,
                    stage: progress.stage,
                };
                bump(&mut state);
            },
            cancel,
        )
        .map_err(install_failure_message)
    }

    fn recheck_resources(&self) {
        {
            let mut state = self.lock();
            if !state.snapshot.enabled || state.recheck {
                return;
            }
            state.recheck = true;
        }
        self.kick_probe();
    }

    fn speak(&self, voice_id: String, rate: SpeechRate) {
        let job = {
            let mut state = self.lock();
            if !state.snapshot.enabled {
                return;
            }
            if state.audio_busy || (!skip_native(self) && host_audio_busy()) {
                state.snapshot.error = Some(LookupError::new(LookupErrorKind::AudioBusy));
                bump(&mut state);
                if state.snapshot.speaking.take().is_some() {
                    state.speech_generation = state.speech_generation.saturating_add(1);
                    bump(&mut state);
                }
                None
            } else if !state
                .snapshot
                .voices
                .iter()
                .any(|voice| voice.id == voice_id)
            {
                return;
            } else {
                speak_text(&state.snapshot).map(str::to_owned).map(|text| {
                    state.speech_generation = state.speech_generation.saturating_add(1);
                    (text, state.speech_generation, voice_id, rate)
                })
            }
        };
        let Some((text, token, voice_id, rate)) = job else {
            self.native_stop();
            return;
        };
        self.native_stop();
        {
            let state = self.lock();
            if state.speech_generation != token || state.audio_busy || !state.snapshot.enabled {
                drop(state);
                self.native_stop();
                return;
            }
        }
        if self.native_speak(&text, &voice_id, rate).is_err() {
            let stop = {
                let mut state = self.lock();
                if state.speech_generation == token {
                    state.speech_generation = state.speech_generation.saturating_add(1);
                    if state.snapshot.speaking.take().is_some() {
                        bump(&mut state);
                    }
                    true
                } else {
                    false
                }
            };
            if stop {
                self.native_stop();
            }
            return;
        }
        {
            let mut state = self.lock();
            if state.speech_generation != token || state.audio_busy || !state.snapshot.enabled {
                drop(state);
                self.native_stop();
                return;
            }
            state.speaking_token = token;
            state.snapshot.speaking = Some(Speaking {
                text,
                voice_id,
                rate,
            });
            bump(&mut state);
        }
    }

    fn stop_speech(&self) {
        {
            let mut state = self.lock();
            state.speech_generation = state.speech_generation.saturating_add(1);
            if state.snapshot.speaking.take().is_some() {
                bump(&mut state);
            }
        }
        self.native_stop();
    }

    fn set_audio_busy(&self, busy: bool) {
        {
            let mut state = self.lock();
            state.audio_busy = busy;
            if busy {
                state.speech_generation = state.speech_generation.saturating_add(1);
                if state.snapshot.speaking.take().is_some() {
                    bump(&mut state);
                }
            }
        }
        if busy {
            self.native_stop();
        }
    }

    fn native_speak(
        &self,
        text: &str,
        voice_id: &str,
        rate: SpeechRate,
    ) -> Result<(), LookupError> {
        #[cfg(test)]
        if self.manual {
            let _ = (text, voice_id, rate);
            let mut speech = self
                .speech
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let token = speech.enqueue(super::speech::SpeechCommand::Start);
            if speech.hold_main {
                speech.fence.cancel(token);
                return Err(LookupError::new(LookupErrorKind::SystemUnavailable));
            }
            speech.run_queue();
            if speech.fence.is_live(token) {
                return Ok(());
            }
            return Err(LookupError::new(LookupErrorKind::SystemUnavailable));
        }
        super::apple::speak(text, voice_id, rate)
    }

    fn native_stop(&self) {
        #[cfg(test)]
        if self.manual {
            let mut speech = self
                .speech
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let token = speech.enqueue(super::speech::SpeechCommand::Stop);
            if speech.hold_main {
                let _ = token;
                return;
            }
            speech.run_queue();
            return;
        }
        super::apple::stop_speech();
    }

    fn native_speaking(&self) -> bool {
        #[cfg(test)]
        if self.manual {
            return self
                .speech
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .speaking;
        }
        super::apple::is_speaking()
    }
}

pub fn configure(config: Config) {
    service().configure(config);
}

pub fn snapshot() -> Snapshot {
    service().snapshot()
}

pub fn edit(text: String) {
    service().edit(text);
}

pub fn submit_local() {
    service().submit_local();
}

pub fn cancel() {
    service().cancel();
}

pub fn prepare_online() -> Result<CompatiblePreview, LookupError> {
    service().prepare_online()
}

pub fn confirm_online(preview: CompatiblePreview) {
    service().confirm_online(preview);
}

pub fn install_dictionary() {
    service().install_dictionary();
}

pub fn cancel_install() {
    service().cancel_install();
}

pub fn recheck_resources() {
    service().recheck_resources();
}

pub fn speak(voice_id: String, rate: SpeechRate) {
    service().speak(voice_id, rate);
}

pub fn stop_speech() {
    service().stop_speech();
}

pub fn set_audio_busy(busy: bool) {
    service().set_audio_busy(busy);
}

fn speak_text(snapshot: &Snapshot) -> Option<&str> {
    snapshot
        .outcome
        .as_ref()
        .map(Outcome::spoken_text)
        .or_else(|| {
            let text = snapshot.text.trim();
            (!text.is_empty()).then_some(text)
        })
}

fn fresh_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn cancel_local(state: &mut State) {
    state.local_cancel.store(true, Ordering::Release);
    state.local_cancel = fresh_cancel();
    state.local_job = None;
}

fn cancel_online(state: &mut State) {
    state.online_cancel.store(true, Ordering::Release);
    state.online_cancel = fresh_cancel();
    state.online_job = None;
    state.snapshot.online_preview = None;
}

fn stop_install(state: &mut State) {
    state.install_cancel.store(true, Ordering::Release);
}

fn invalidate(state: &mut State, halt_install: bool) {
    cancel_local(state);
    cancel_online(state);
    if halt_install {
        stop_install(state);
    }
    state.speech_generation = state.speech_generation.saturating_add(1);
    state.snapshot.outcome = None;
    state.snapshot.error = None;
    state.snapshot.online_preview = None;
    state.snapshot.speaking = None;
    state.snapshot.generation = state.snapshot.generation.saturating_add(1);
}

fn bump(state: &mut State) {
    state.snapshot.revision = state.snapshot.revision.saturating_add(1);
    super::notify_changed();
}

fn same_config(a: &Config, b: &Config) -> bool {
    a.enabled == b.enabled
        && a.compatible_base_url == b.compatible_base_url
        && a.compatible_model == b.compatible_model
        && a.compatible_credential_path == b.compatible_credential_path
        && a.compatible_deadline == b.compatible_deadline
}

fn compatible_changed(a: &Config, b: &Config) -> bool {
    a.compatible_base_url != b.compatible_base_url
        || a.compatible_model != b.compatible_model
        || a.compatible_credential_path != b.compatible_credential_path
}

fn confirm_binding(state: &State, preview: &CompatiblePreview) -> bool {
    if !state.snapshot.enabled || state.snapshot.phase != Phase::OnlinePreview {
        return false;
    }
    let Some(stored) = state.snapshot.online_preview.as_ref() else {
        return false;
    };
    if stored != preview {
        return false;
    }
    let Ok(text) = normalize_submit(&state.snapshot.text) else {
        return false;
    };
    text == preview.text
        && preview.base_url == state.config.compatible_base_url
        && preview.model == state.config.compatible_model
}

fn already_in_flight(state: &State, text: &str) -> bool {
    match state.snapshot.phase {
        Phase::LocalPending => state.snapshot.text == text,
        Phase::Ready => {
            state.snapshot.error.is_none()
                && state.snapshot.outcome.is_some()
                && normalize_submit(&state.snapshot.text).ok().as_deref() == Some(text)
        }
        _ => false,
    }
}

fn live_local(engine: &Engine, job: &LocalJob) -> bool {
    if job.cancel.load(Ordering::Acquire) {
        return false;
    }
    let state = engine.lock();
    state.snapshot.enabled
        && state.snapshot.generation == job.generation
        && state.snapshot.phase == Phase::LocalPending
        && state.snapshot.text == job.text
}

fn live_online(engine: &Engine, job: &OnlineJob) -> bool {
    if job.cancel.load(Ordering::Acquire) {
        return false;
    }
    let state = engine.lock();
    state.snapshot.enabled
        && state.snapshot.generation == job.preview.generation
        && state.snapshot.phase == Phase::OnlinePending
        && state.snapshot.text == job.preview.text
}

fn run_local(engine: &Engine, job: LocalJob) {
    if !live_local(engine, &job) {
        return;
    }
    let dictionary = loaded_dictionary(engine);
    if !live_local(engine, &job) {
        return;
    }
    let routed = route(&job.text, dictionary.as_deref());
    let result = match routed {
        Err(error) => Err(error),
        Ok(Route::Word { entry, mapped_from }) => {
            let source = if mapped_from.is_some() {
                Source::DictionaryLemma
            } else {
                Source::Dictionary
            };
            Ok(Outcome::Word(WordResult {
                query: job.text.clone(),
                headword: entry.headword.to_string(),
                mapped_from,
                phonetic: entry.phonetic.map(|value| value.to_string()),
                senses: senses(&entry.translation, &entry.pos),
                source,
            }))
        }
        Ok(Route::NeedSystem { as_word }) => {
            if !live_local(engine, &job) {
                return;
            }
            system_outcome(&job.text, as_word)
        }
    };
    if !live_local(engine, &job) {
        return;
    }
    let mut state = engine.lock();
    if !state.snapshot.enabled
        || state.snapshot.generation != job.generation
        || state.snapshot.phase != Phase::LocalPending
        || state.snapshot.text != job.text
        || job.cancel.load(Ordering::Acquire)
    {
        return;
    }
    match result {
        Ok(outcome) => {
            state.snapshot.outcome = Some(outcome);
            state.snapshot.error = None;
            state.snapshot.phase = Phase::Ready;
        }
        Err(error) => {
            state.snapshot.outcome = None;
            state.snapshot.error = Some(error);
            state.snapshot.phase = Phase::Ready;
        }
    }
    bump(&mut state);
}

fn system_outcome(text: &str, as_word: bool) -> Result<Outcome, LookupError> {
    let translated = super::apple::translate(text)?;
    if as_word {
        Ok(Outcome::Word(WordResult {
            query: text.to_owned(),
            headword: text.to_owned(),
            mapped_from: None,
            phonetic: None,
            senses: vec![super::Sense {
                pos: None,
                gloss: translated,
            }],
            source: Source::System,
        }))
    } else {
        Ok(Outcome::Phrase(PhraseResult {
            query: text.to_owned(),
            translation: translated,
            source: Source::System,
        }))
    }
}

fn run_online(engine: &Engine, job: OnlineJob) {
    if !live_online(engine, &job) {
        return;
    }
    let result = super::compatible::send(
        &job.preview,
        job.credential.as_deref(),
        job.deadline,
        &job.cancel,
    );
    if !live_online(engine, &job) {
        return;
    }
    let mut state = engine.lock();
    if !state.snapshot.enabled
        || state.snapshot.generation != job.preview.generation
        || state.snapshot.phase != Phase::OnlinePending
        || state.snapshot.text != job.preview.text
        || job.cancel.load(Ordering::Acquire)
    {
        return;
    }
    match result {
        Ok(translation) => {
            let outcome = if is_single_token(&job.preview.text) {
                Outcome::Word(WordResult {
                    query: job.preview.text.clone(),
                    headword: job.preview.text.clone(),
                    mapped_from: None,
                    phonetic: None,
                    senses: vec![super::Sense {
                        pos: None,
                        gloss: translation,
                    }],
                    source: Source::Compatible,
                })
            } else {
                Outcome::Phrase(PhraseResult {
                    query: job.preview.text.clone(),
                    translation,
                    source: Source::Compatible,
                })
            };
            state.snapshot.outcome = Some(outcome);
            state.snapshot.error = None;
            state.snapshot.phase = Phase::Ready;
        }
        Err(error) if error.kind == LookupErrorKind::Canceled => {
            state.snapshot.phase = Phase::Idle;
        }
        Err(error) => {
            state.snapshot.error = Some(error);
            state.snapshot.phase = Phase::Ready;
        }
    }
    bump(&mut state);
}

fn install_failure_message(error: color_eyre::eyre::Report) -> String {
    let text = error.to_string();
    let first = text.lines().next().unwrap_or(text.as_str()).trim();
    if first.is_empty() {
        "Dictionary install failed".into()
    } else {
        first.chars().take(200).collect()
    }
}

fn is_cancel_message(message: &str) -> bool {
    message.to_ascii_lowercase().contains("canceled")
}

fn refresh_resources(engine: &Engine) {
    {
        let state = engine.lock();
        if !state.snapshot.enabled {
            return;
        }
    }
    let dictionary = super::dictionary::status_on_disk();
    let (system, voices) = if skip_native(engine) {
        let state = engine.lock();
        (
            state.snapshot.resources.system.clone(),
            state.snapshot.voices.clone(),
        )
    } else {
        (super::apple::system_status(), super::apple::voices())
    };
    let mut state = engine.lock();
    if !state.snapshot.enabled {
        return;
    }
    if !matches!(
        state.snapshot.resources.dictionary,
        DictionaryStatus::Installing { .. }
    ) {
        state.snapshot.resources.dictionary = dictionary;
    }
    state.snapshot.resources.system = system;
    state.snapshot.voices = voices;
    bump(&mut state);
}

fn host_audio_busy() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::audio::recording_environment::translation_audio_hold_count() > 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn skip_native(engine: &Engine) -> bool {
    #[cfg(test)]
    {
        engine.manual
    }
    #[cfg(not(test))]
    {
        let _ = engine;
        false
    }
}

fn loaded_dictionary(engine: &Engine) -> Option<Arc<Dictionary>> {
    {
        let state = engine.lock();
        if !state.snapshot.enabled {
            return None;
        }
        if let Some(dictionary) = &state.dictionary {
            return Some(Arc::clone(dictionary));
        }
    }
    let loaded = super::dictionary::load_keyed().ok().flatten();
    let mut state = engine.lock();
    if !state.snapshot.enabled {
        return None;
    }
    if let Some(dictionary) = &state.dictionary {
        return Some(Arc::clone(dictionary));
    }
    let loaded = loaded?;
    let current = super::dictionary::current_key().ok().flatten();
    if current.as_deref() != Some(loaded.key.as_str()) {
        return None;
    }
    let dictionary = Arc::new(loaded.dictionary);
    state.dictionary = Some(Arc::clone(&dictionary));
    state.dictionary_key = Some(loaded.key);
    Some(dictionary)
}

#[cfg(test)]
impl Engine {
    fn manual() -> Self {
        let (tx, _rx) = mpsc::channel();
        let (probe_tx, _probe_rx) = mpsc::channel();
        let (install_tx, _install_rx) = mpsc::channel();
        Self {
            state: Arc::new(Mutex::new(State {
                snapshot: Snapshot::idle(),
                config: Config::default(),
                audio_busy: false,
                local_cancel: Arc::new(AtomicBool::new(false)),
                online_cancel: Arc::new(AtomicBool::new(false)),
                install_cancel: Arc::new(AtomicBool::new(false)),
                install_generation: 0,
                install_pending: false,
                install_running: false,
                speech_generation: 0,
                speaking_token: 0,
                local_job: None,
                online_job: None,
                recheck: false,
                dictionary: None,
                dictionary_key: None,
            })),
            tx,
            probe_tx,
            install_tx,
            wake: Arc::new(AtomicBool::new(false)),
            probe_wake: Arc::new(AtomicBool::new(false)),
            install_wake: Arc::new(AtomicBool::new(false)),
            manual: true,
            speech: Arc::new(Mutex::new(FakeSpeech {
                fence: super::speech::SpeechFence::default(),
                queue: std::collections::VecDeque::new(),
                hold_main: false,
                speaking: false,
                starts: 0,
                stops: 0,
            })),
            install_hook: Arc::new(Mutex::new(None)),
        }
    }

    fn pump(&self) {
        self.run_turn();
        if self.lock().recheck || self.probe_wake.load(Ordering::Acquire) {
            self.run_probe();
        }
        if self.install_wake.load(Ordering::Acquire) || self.lock().install_pending {
            self.run_install_turn();
        }
    }

    fn hold_speech_main(&self, hold: bool) {
        self.speech
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .hold_main = hold;
    }

    fn flush_speech_main(&self) {
        self.speech
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .run_queue();
    }

    fn set_install_hook(&self, hook: InstallHook) {
        *self
            .install_hook
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(hook);
    }

    fn steal_local(&self) -> Option<LocalJob> {
        self.lock().local_job.take()
    }

    fn steal_online(&self) -> Option<OnlineJob> {
        self.lock().online_job.take()
    }

    fn replace_dictionary(&self, dictionary: Dictionary) {
        let mut state = self.lock();
        state.dictionary = Some(Arc::new(dictionary));
        state.dictionary_key = Some("test".into());
    }

    fn set_voices(&self, voices: Vec<super::VoiceInfo>) {
        let mut state = self.lock();
        state.snapshot.voices = voices;
        bump(&mut state);
    }

    fn enable_with_dict(&self) {
        self.configure(Config {
            enabled: true,
            compatible_deadline: Duration::from_secs(3),
            ..Config::default()
        });
        self.replace_dictionary(test_dictionary());
    }
}

#[cfg(test)]
impl FakeSpeech {
    fn enqueue(&mut self, command: super::speech::SpeechCommand) -> u64 {
        let token = self.fence.issue(command);
        self.queue.push_back(QueuedSpeech { token, command });
        token
    }

    fn run_queue(&mut self) {
        while let Some(cmd) = self.queue.pop_front() {
            if !self.fence.is_live(cmd.token) {
                continue;
            }
            match cmd.command {
                super::speech::SpeechCommand::Start => {
                    self.speaking = true;
                    self.starts += 1;
                }
                super::speech::SpeechCommand::Stop => {
                    self.speaking = false;
                    self.stops += 1;
                }
            }
        }
    }
}

#[cfg(test)]
fn test_dictionary() -> Dictionary {
    use super::dictionary::Entry;
    Dictionary::from_parts(
        [
            (
                "hello",
                Entry {
                    headword: "hello".into(),
                    phonetic: Some("hә'lәu".into()),
                    translation: "interj. 喂".into(),
                    pos: String::new(),
                },
            ),
            (
                "give",
                Entry {
                    headword: "give".into(),
                    phonetic: Some("giv".into()),
                    translation: "vt. 给".into(),
                    pos: String::new(),
                },
            ),
            (
                "take off",
                Entry {
                    headword: "take off".into(),
                    phonetic: None,
                    translation: "起飞".into(),
                    pos: String::new(),
                },
            ),
        ],
        [("gave", "give")],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::{Accent, MAX_QUERY_CHARS, VoiceInfo};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn local_lookup_generation_and_cancel_drop_stale_results() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.edit("hello".into());
        engine.submit_local();
        engine.pump();
        let ready = engine.snapshot();
        let Outcome::Word(word) = ready.outcome.unwrap() else {
            panic!("hello should be a word");
        };
        assert_eq!(word.headword, "hello");
        assert_eq!(word.phonetic.as_deref(), Some("hә'lәu"));
        assert_eq!(word.source, Source::Dictionary);
        let generation = ready.generation;

        engine.edit("gave".into());
        engine.submit_local();
        engine.pump();
        let mapped = engine.snapshot();
        let Outcome::Word(word) = mapped.outcome.unwrap() else {
            panic!("gave should map to give");
        };
        assert_eq!(word.headword, "give");
        assert_eq!(word.mapped_from.as_deref(), Some("gave"));
        assert_eq!(word.source, Source::DictionaryLemma);
        assert_ne!(mapped.generation, generation);

        engine.edit("hello".into());
        engine.submit_local();
        engine.cancel();
        let canceled = engine.snapshot();
        assert_eq!(canceled.phase, Phase::Idle);
        assert!(canceled.outcome.is_none());

        engine.edit("take off".into());
        engine.submit_local();
        engine.pump();
        let phrase = engine.snapshot();
        let Outcome::Word(word) = phrase.outcome.unwrap() else {
            panic!("take off is in ECDICT as a multiword headword");
        };
        assert_eq!(word.headword, "take off");
        assert!(word.phonetic.is_none());

        engine.edit("a".repeat(MAX_QUERY_CHARS + 1));
        engine.submit_local();
        let too_long = engine.snapshot();
        assert_eq!(
            too_long.error.as_ref().map(|error| error.kind),
            Some(LookupErrorKind::TooLong)
        );

        engine.configure(Config::default());
        assert!(!engine.snapshot().enabled);
        assert!(engine.snapshot().text.is_empty());
    }

    #[test]
    fn confirm_online_sends_only_the_captured_preview() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                if String::from_utf8_lossy(&request).contains("take off") {
                    break;
                }
            }
            let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"起飞"}}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let engine = Engine::manual();
        engine.configure(Config {
            enabled: true,
            compatible_base_url: format!("http://{addr}/v1"),
            compatible_model: "test-model".into(),
            compatible_credential_path: String::new(),
            compatible_deadline: Duration::from_secs(3),
        });
        engine.replace_dictionary(Dictionary::from_parts([], []));
        engine.edit("hello".into());
        let stale = engine.prepare_online().unwrap();
        engine.edit("take off".into());
        engine.confirm_online(stale);
        assert_ne!(engine.snapshot().phase, Phase::OnlinePending);

        let preview = engine.prepare_online().unwrap();
        assert_eq!(preview.text, "take off");
        engine.confirm_online(preview);
        engine.pump();
        let ready = engine.snapshot();
        assert_eq!(ready.phase, Phase::Ready);
        match ready.outcome.unwrap() {
            Outcome::Phrase(phrase) => {
                assert_eq!(phrase.translation, "起飞");
                assert_eq!(phrase.source, Source::Compatible);
            }
            Outcome::Word(word) => {
                assert_eq!(word.senses[0].gloss, "起飞");
                assert!(word.phonetic.is_none());
                assert_eq!(word.source, Source::Compatible);
            }
        }
        server.join().unwrap();
    }

    #[test]
    fn voices_only_label_english_accents() {
        for voice in super::super::apple::voices() {
            assert!(matches!(voice.accent, Accent::Us | Accent::Gb));
        }
    }

    #[test]
    fn confirm_online_consumes_stored_preview_once() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.configure(Config {
            enabled: true,
            compatible_base_url: "http://127.0.0.1/v1".into(),
            compatible_model: "flash".into(),
            compatible_credential_path: String::new(),
            compatible_deadline: Duration::from_secs(3),
        });
        engine.replace_dictionary(test_dictionary());
        engine.edit("hello".into());
        let preview = engine.prepare_online().unwrap();
        assert_eq!(engine.snapshot().phase, Phase::OnlinePreview);
        engine.confirm_online(preview.clone());
        let pending = engine.snapshot();
        assert_eq!(pending.phase, Phase::OnlinePending);
        assert!(pending.online_preview.is_none());
        let job = engine.steal_online().expect("confirm should queue one job");
        assert_eq!(job.preview, preview);
        assert!(job.credential.is_none());
        let revision = engine.snapshot().revision;
        engine.confirm_online(preview);
        assert!(engine.steal_online().is_none());
        assert_eq!(engine.snapshot().revision, revision);
        assert_eq!(engine.snapshot().phase, Phase::OnlinePending);
    }

    #[test]
    fn confirm_online_rejects_phase_and_config_mismatch() {
        let engine = Engine::manual();
        engine.configure(Config {
            enabled: true,
            compatible_base_url: "http://127.0.0.1/v1".into(),
            compatible_model: "flash".into(),
            compatible_credential_path: String::new(),
            compatible_deadline: Duration::from_secs(3),
        });
        engine.replace_dictionary(test_dictionary());
        engine.edit("hello".into());
        engine.submit_local();
        let preview = engine.prepare_online().unwrap();
        let mut tampered = preview.clone();
        tampered.model = "other".into();
        engine.confirm_online(tampered);
        assert_eq!(engine.snapshot().phase, Phase::OnlinePreview);
        assert!(engine.steal_online().is_none());

        engine.configure(Config {
            enabled: true,
            compatible_base_url: "http://127.0.0.1/v2".into(),
            compatible_model: "flash".into(),
            compatible_credential_path: String::new(),
            compatible_deadline: Duration::from_secs(3),
        });
        assert_ne!(engine.snapshot().phase, Phase::OnlinePreview);
        engine.confirm_online(preview);
        assert!(engine.steal_online().is_none());
    }

    #[test]
    fn config_change_cancels_bound_online_job() {
        let engine = Engine::manual();
        engine.configure(Config {
            enabled: true,
            compatible_base_url: "http://127.0.0.1/v1".into(),
            compatible_model: "flash".into(),
            compatible_credential_path: String::new(),
            compatible_deadline: Duration::from_secs(3),
        });
        engine.replace_dictionary(test_dictionary());
        engine.edit("hello".into());
        let preview = engine.prepare_online().unwrap();
        engine.confirm_online(preview);
        let job = engine.steal_online().unwrap();
        assert!(job.credential.is_none());
        engine.configure(Config {
            enabled: true,
            compatible_base_url: "http://127.0.0.1/v1".into(),
            compatible_model: "flash".into(),
            compatible_credential_path: "b.json".into(),
            compatible_deadline: Duration::from_secs(3),
        });
        assert!(job.cancel.load(Ordering::Acquire));
        assert_ne!(engine.snapshot().phase, Phase::OnlinePending);
        run_online(&engine, job);
        assert!(engine.snapshot().outcome.is_none());
    }

    #[test]
    fn stale_local_does_not_commit_after_edit_or_online() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.edit("hello".into());
        engine.submit_local();
        let stale = engine.steal_local().unwrap();
        engine.edit("gave".into());
        run_local(&engine, stale);
        assert!(engine.snapshot().outcome.is_none());
        assert_eq!(engine.snapshot().phase, Phase::Idle);

        engine.submit_local();
        let local = engine.steal_local().unwrap();
        engine.configure(Config {
            enabled: true,
            compatible_base_url: "http://127.0.0.1/v1".into(),
            compatible_model: "flash".into(),
            compatible_credential_path: String::new(),
            compatible_deadline: Duration::from_secs(3),
        });
        engine.replace_dictionary(test_dictionary());
        let preview = engine.prepare_online().unwrap();
        engine.confirm_online(preview);
        run_local(&engine, local);
        assert_eq!(engine.snapshot().phase, Phase::OnlinePending);
        match engine.snapshot().outcome {
            Some(Outcome::Word(word)) => panic!("stale local committed {}", word.headword),
            Some(Outcome::Phrase(phrase)) => panic!("stale local committed {}", phrase.query),
            None => {}
        }
    }

    #[test]
    fn old_cancel_token_stays_set_when_new_work_starts() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.edit("hello".into());
        engine.submit_local();
        let first = engine.steal_local().unwrap();
        engine.cancel();
        engine.edit("gave".into());
        engine.submit_local();
        let second = engine.steal_local().unwrap();
        assert!(first.cancel.load(Ordering::Acquire));
        assert!(!second.cancel.load(Ordering::Acquire));
        assert!(!Arc::ptr_eq(&first.cancel, &second.cancel));
        run_local(&engine, first);
        assert!(engine.snapshot().outcome.is_none());
        run_local(&engine, second);
        let Outcome::Word(word) = engine.snapshot().outcome.unwrap() else {
            panic!("gave should map");
        };
        assert_eq!(word.headword, "give");
    }

    #[test]
    fn configure_unchanged_is_noop_and_recheck_coalesces() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.pump();
        let after_enable = engine.snapshot();
        engine.configure(Config {
            enabled: true,
            compatible_deadline: Duration::from_secs(3),
            ..Config::default()
        });
        assert_eq!(engine.snapshot().revision, after_enable.revision);
        engine.recheck_resources();
        engine.recheck_resources();
        engine.recheck_resources();
        assert!(engine.lock().recheck);
        engine.pump();
        assert!(!engine.lock().recheck);
        engine.recheck_resources();
        assert!(engine.lock().recheck);
    }

    #[test]
    fn disabled_startup_does_not_queue_work() {
        let engine = Engine::manual();
        assert!(!engine.snapshot().enabled);
        assert!(!engine.lock().recheck);
        assert!(engine.lock().dictionary.is_none());
        engine.edit("hello".into());
        engine.submit_local();
        assert!(engine.steal_local().is_none());
        engine.recheck_resources();
        assert!(!engine.lock().recheck);
        assert!(engine.lock().dictionary.is_none());
    }

    #[test]
    fn speech_stop_and_busy_fence_stale_starts() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.set_voices(vec![VoiceInfo {
            id: "com.apple.voice.en-US".into(),
            name: "Samantha".into(),
            accent: Accent::Us,
        }]);
        engine.edit("hello".into());
        engine.submit_local();
        engine.pump();
        engine.speak("missing".into(), SpeechRate::Normal);
        assert!(engine.snapshot().speaking.is_none());
        assert_eq!(engine.speech.lock().unwrap().starts, 0);

        engine.speak("com.apple.voice.en-US".into(), SpeechRate::Normal);
        assert_eq!(
            engine
                .snapshot()
                .speaking
                .as_ref()
                .map(|s| s.voice_id.as_str()),
            Some("com.apple.voice.en-US")
        );
        engine.stop_speech();
        assert!(engine.snapshot().speaking.is_none());
        assert!(engine.speech.lock().unwrap().stops >= 1);

        engine.speak("com.apple.voice.en-US".into(), SpeechRate::Normal);
        engine.set_audio_busy(true);
        assert!(engine.snapshot().speaking.is_none());
        let starts = engine.speech.lock().unwrap().starts;
        engine.speak("com.apple.voice.en-US".into(), SpeechRate::Normal);
        assert!(engine.snapshot().speaking.is_none());
        assert_eq!(engine.speech.lock().unwrap().starts, starts);
    }

    #[test]
    fn late_main_start_timeout_then_stop_then_new_start() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.set_voices(vec![VoiceInfo {
            id: "com.apple.voice.en-US".into(),
            name: "Samantha".into(),
            accent: Accent::Us,
        }]);
        engine.edit("hello".into());
        engine.submit_local();
        engine.pump();
        engine.hold_speech_main(true);
        engine.speak("com.apple.voice.en-US".into(), SpeechRate::Normal);
        assert!(engine.snapshot().speaking.is_none());
        assert_eq!(engine.speech.lock().unwrap().starts, 0);
        engine.stop_speech();
        engine.hold_speech_main(false);
        engine.speak("com.apple.voice.en-US".into(), SpeechRate::Normal);
        engine.flush_speech_main();
        assert_eq!(engine.speech.lock().unwrap().starts, 1);
        assert_eq!(
            engine
                .snapshot()
                .speaking
                .as_ref()
                .map(|speaking| speaking.voice_id.as_str()),
            Some("com.apple.voice.en-US")
        );
    }

    #[test]
    fn failed_speak_does_not_publish() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        engine.set_voices(vec![VoiceInfo {
            id: "com.apple.voice.en-US".into(),
            name: "Samantha".into(),
            accent: Accent::Us,
        }]);
        engine.edit("hello".into());
        engine.hold_speech_main(true);
        engine.speak("com.apple.voice.en-US".into(), SpeechRate::Normal);
        assert!(engine.snapshot().speaking.is_none());
        engine.flush_speech_main();
        assert_eq!(engine.speech.lock().unwrap().starts, 0);
        assert!(engine.snapshot().speaking.is_none());
    }

    #[test]
    fn installer_duplicate_cancel_retry_and_failure_visibility() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        let started = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
        engine.set_install_hook(Arc::new({
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let calls = Arc::clone(&calls);
            move |cancel: &AtomicBool| {
                calls.fetch_add(1, Ordering::SeqCst);
                {
                    let mut flag = started.0.lock().unwrap_or_else(|error| error.into_inner());
                    *flag = true;
                    started.1.notify_all();
                }
                let mut go = release.0.lock().unwrap_or_else(|error| error.into_inner());
                while !*go {
                    go = release
                        .1
                        .wait(go)
                        .unwrap_or_else(|error| error.into_inner());
                }
                if cancel.load(Ordering::Acquire) {
                    Err("download canceled".into())
                } else {
                    Err("checksum mismatch".into())
                }
            }
        }));
        engine.install_dictionary();
        let worker = engine.clone_worker();
        let handle = thread::spawn(move || worker.pump());
        {
            let mut flag = started.0.lock().unwrap_or_else(|error| error.into_inner());
            while !*flag {
                flag = started
                    .1
                    .wait(flag)
                    .unwrap_or_else(|error| error.into_inner());
            }
        }
        engine.install_dictionary();
        engine.install_dictionary();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        engine.cancel_install();
        engine.cancel_install();
        {
            let mut go = release.0.lock().unwrap_or_else(|error| error.into_inner());
            *go = true;
            release.1.notify_all();
        }
        handle.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!matches!(
            engine.snapshot().resources.dictionary,
            DictionaryStatus::Installing { .. }
        ));

        engine.set_install_hook(Arc::new(|_cancel: &AtomicBool| {
            Err("checksum mismatch".into())
        }));
        engine.install_dictionary();
        engine.pump();
        match engine.snapshot().resources.dictionary {
            DictionaryStatus::Failed { message } => {
                assert!(message.contains("checksum mismatch"), "{message}");
            }
            other => panic!("genuine failure must be visible, got {other:?}"),
        }
        engine.replace_dictionary(test_dictionary());
        engine.edit("hello".into());
        engine.submit_local();
        engine.pump();
        assert!(engine.snapshot().outcome.is_some());

        let before = calls.load(Ordering::SeqCst);
        engine.set_install_hook(Arc::new({
            let calls = Arc::clone(&calls);
            move |_cancel: &AtomicBool| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(DictionaryStatus::Ready { entries: 3 })
            }
        }));
        engine.install_dictionary();
        engine.pump();
        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        match engine.snapshot().resources.dictionary {
            DictionaryStatus::Ready { entries } => assert_eq!(entries, 3),
            other => panic!("retry must commit, got {other:?}"),
        }
    }

    #[test]
    fn installer_cancel_right_before_commit() {
        let engine = Engine::manual();
        engine.enable_with_dict();
        let at_commit = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let go = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        engine.set_install_hook(Arc::new({
            let at_commit = Arc::clone(&at_commit);
            let go = Arc::clone(&go);
            move |cancel: &AtomicBool| {
                {
                    let mut flag = at_commit
                        .0
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    *flag = true;
                    at_commit.1.notify_all();
                }
                let mut release = go.0.lock().unwrap_or_else(|error| error.into_inner());
                while !*release {
                    release =
                        go.1.wait(release)
                            .unwrap_or_else(|error| error.into_inner());
                }
                if cancel.load(Ordering::Acquire) {
                    Err("download canceled".into())
                } else {
                    Ok(DictionaryStatus::Ready { entries: 9 })
                }
            }
        }));
        engine.install_dictionary();
        let worker = engine.clone_worker();
        let handle = thread::spawn(move || worker.pump());
        {
            let mut flag = at_commit
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while !*flag {
                flag = at_commit
                    .1
                    .wait(flag)
                    .unwrap_or_else(|error| error.into_inner());
            }
        }
        engine.cancel_install();
        {
            let mut release = go.0.lock().unwrap_or_else(|error| error.into_inner());
            *release = true;
            go.1.notify_all();
        }
        handle.join().unwrap();
        match engine.snapshot().resources.dictionary {
            DictionaryStatus::Ready { entries: 9 } => {
                panic!("cancel that wins must prevent commit")
            }
            DictionaryStatus::Installing { .. } => panic!("must not stay Installing"),
            _ => {}
        }
    }

    #[cfg(unix)]
    #[test]
    fn confirm_online_captures_credential_at_confirm() {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!(
            "hex-translation-cred-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        write!(
            file,
            "{{\"url\":\"http://127.0.0.1/v1\",\"key\":\"test-key-one\"}}"
        )
        .unwrap();
        drop(file);
        let engine = Engine::manual();
        engine.configure(Config {
            enabled: true,
            compatible_base_url: "http://127.0.0.1/v1".into(),
            compatible_model: "flash".into(),
            compatible_credential_path: path.to_str().unwrap().into(),
            compatible_deadline: Duration::from_secs(3),
        });
        engine.replace_dictionary(test_dictionary());
        engine.edit("hello".into());
        let preview = engine.prepare_online().unwrap();
        engine.confirm_online(preview);
        let job = engine.steal_online().expect("confirm should capture");
        assert_eq!(job.credential.as_deref(), Some("test-key-one"));
        std::fs::write(
            &path,
            b"{\"url\":\"http://127.0.0.1/v1\",\"key\":\"test-key-two\"}",
        )
        .unwrap();
        assert_eq!(job.credential.as_deref(), Some("test-key-one"));
        let _ = std::fs::remove_file(path);
    }
}
