// The sound that a headset app places in its subspace, as the frame loop hands it to the audio
// player: where the emitter of each source stands relative to the head. It also tests when the app
// is silent.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use cranpose::{
    AudioClip, AudioError, AudioPlayer, DistanceAttenuation, EmitterId, EmitterReading,
    LifecycleState, MAX_EMITTERS, PlaybackParams, SoundId, SpatialScene, VoiceId, default_audio,
    prelude::*,
};
use cranpose_core::{CompositionLocalProvider, rememberMutableStateOf};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    FollowMode, FollowTarget, MovePolicy, PointSourceParams, SpatialMoveEventType, SpatialPanel,
    SpatialPointSource, SpatialVolume, SubspaceModifier,
    sound_sources::SoundPlace,
    space::{Origin, PanelMove, local_space},
};

// What a voice that the stage started asked the player for.
#[derive(Clone, Copy, Debug)]
struct Played {
    sound: SoundId,
    params: PlaybackParams,
    looping: bool,
}

// A player that records the voices that it is asked to start and holds a spatial scene.
struct RecordingPlayer {
    scene: Option<Arc<SpatialScene>>,
    played: Mutex<Vec<Played>>,
}

impl RecordingPlayer {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            scene: Some(Arc::new(SpatialScene::new())),
            played: Mutex::new(Vec::new()),
        })
    }

    fn played(&self) -> Vec<Played> {
        self.played.lock().expect("no panic holds the lock").clone()
    }

    fn scene(&self) -> Arc<SpatialScene> {
        Arc::clone(self.scene.as_ref().expect("this player has a scene"))
    }

    fn record(&self, sound: SoundId, params: PlaybackParams, looping: bool) {
        self.played
            .lock()
            .expect("no panic holds the lock")
            .push(Played {
                sound,
                params,
                looping,
            });
    }
}

impl AudioPlayer for RecordingPlayer {
    fn load_clip(&self, _clip: AudioClip) -> Result<SoundId, AudioError> {
        Ok(SoundId::from_raw(1))
    }
    fn play(&self, id: SoundId, params: PlaybackParams) {
        self.record(id, params, false);
    }
    fn play_loop(&self, id: SoundId, params: PlaybackParams) -> VoiceId {
        self.record(id, params, true);
        VoiceId::from_raw(1)
    }
    fn stop(&self, _id: SoundId) {}
    fn stop_voice(&self, _voice: VoiceId) {}
    fn set_master_volume(&self, _volume: f32) {}
    fn spatial_scene(&self) -> Option<Arc<SpatialScene>> {
        self.scene.clone()
    }
}

fn stage(player: &Arc<RecordingPlayer>) -> SoundStage {
    SoundStage::new(Arc::clone(player) as AudioPlayerRef).expect("the player has a scene")
}

fn with_space(test: impl FnOnce(Space)) {
    cranpose_testing::run_test_composition(|_| test(Space::new()));
}

fn looping() -> PointSource {
    PointSource::new(SoundId::from_raw(1)).looping()
}

// The place of the emitter that the voice `played` plays from, in the listener's axes: x to the
// right, y up and z behind.
fn head_relative(scene: &SpatialScene, played: &Played) -> [f32; 3] {
    let id = played.params.emitter.expect("the voice has an emitter");
    let EmitterReading::Placed { position, .. } = scene.read_emitter(id) else {
        panic!("the emitter is gone");
    };
    scene
        .listener()
        .expect("a listener")
        .local(position)
        .map(|meters| (meters * 1e4).round() / 1e4)
}

// The head turned by `degrees` to its left about the vertical axis
fn turned_left(degrees: f32) -> Pose {
    let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
    Pose {
        position: [0.0; 3],
        orientation: [0.0, sin, 0.0, cos],
    }
}

fn emitter_of(played: &Played) -> EmitterId {
    played.params.emitter.expect("the voice has an emitter")
}

#[test]
fn a_source_that_enters_the_space_plays_once_from_an_emitter_at_its_pose() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        let id = space.new_sound_id();
        space.place_point_source(
            id,
            looping(),
            Origin::from(Pose::IDENTITY),
            Pose::at([-0.8, 0.2, -1.0]),
        );
        for _ in 0..3 {
            stage.update(Some(Pose::IDENTITY), &space);
        }
        let played = player.played();
        assert_eq!(
            played.len(),
            1,
            "one voice for one source, however many frames"
        );
        assert!(played[0].looping);
        assert_eq!(played[0].sound, SoundId::from_raw(1));
        assert_eq!(
            head_relative(&player.scene(), &played[0]),
            [-0.8, 0.2, -1.0]
        );
    });
}

#[test]
fn a_source_that_does_not_loop_plays_once_through_play() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        let once = PointSource::new(SoundId::from_raw(1));
        space.place_point_source(
            space.new_sound_id(),
            once,
            Origin::from(Pose::IDENTITY),
            Pose::at([0.0, 0.0, -1.0]),
        );
        stage.update(Some(Pose::IDENTITY), &space);
        assert!(!player.played()[0].looping);
    });
}

#[test]
fn a_source_to_the_left_of_the_head_has_a_negative_x_and_one_to_the_right_a_positive_x() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        for x in [-0.8, 0.8] {
            space.place_point_source(
                space.new_sound_id(),
                looping(),
                Origin::from(Pose::IDENTITY),
                Pose::at([x, 0.0, -1.0]),
            );
        }
        stage.update(Some(Pose::IDENTITY), &space);
        let played = player.played();
        let [left, right] = [0, 1].map(|index| head_relative(&player.scene(), &played[index]));
        assert!(left[0] < -0.7, "left: {left:?}");
        assert!(right[0] > 0.7, "right: {right:?}");
    });
}

#[test]
fn a_head_that_turns_moves_the_source_to_the_other_side() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        // A meter ahead of the head's start
        space.place_point_source(
            space.new_sound_id(),
            looping(),
            Origin::from(Pose::IDENTITY),
            Pose::at([0.0, 0.0, -1.0]),
        );
        stage.update(Some(Pose::IDENTITY), &space);
        let played = player.played()[0];
        assert_eq!(head_relative(&player.scene(), &played), [0.0, 0.0, -1.0]);
        // A head turned 90 degrees to the left looks along -x, so what was ahead is at its right
        stage.update(Some(turned_left(90.0)), &space);
        assert_eq!(head_relative(&player.scene(), &played), [1.0, 0.0, 0.0]);
        // Turned 90 degrees to the right, the same source is at its left
        stage.update(Some(turned_left(-90.0)), &space);
        assert_eq!(head_relative(&player.scene(), &played), [-1.0, 0.0, 0.0]);
        // Turned all the way round, the source is behind the head
        stage.update(Some(turned_left(180.0)), &space);
        assert_eq!(head_relative(&player.scene(), &played), [0.0, 0.0, 1.0]);
    });
}

#[test]
fn a_head_that_walks_changes_the_distance_to_the_source() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        space.place_point_source(
            space.new_sound_id(),
            looping(),
            Origin::from(Pose::IDENTITY),
            Pose::at([0.0, 0.0, -1.0]),
        );
        stage.update(Some(Pose::at([0.0, 0.0, 0.0])), &space);
        let played = player.played()[0];
        assert_eq!(head_relative(&player.scene(), &played), [0.0, 0.0, -1.0]);
        stage.update(Some(Pose::at([0.0, 0.0, -0.5])), &space);
        assert_eq!(head_relative(&player.scene(), &played), [0.0, 0.0, -0.5]);
        // Past the source, it stands behind the head
        stage.update(Some(Pose::at([0.0, 0.0, -3.0])), &space);
        assert_eq!(head_relative(&player.scene(), &played), [0.0, 0.0, 2.0]);
    });
}

#[test]
fn a_source_that_moves_keeps_its_voice_and_its_emitter_follows() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        let id = space.new_sound_id();
        let origin = Origin::from(Pose::IDENTITY);
        space.place_point_source(id, looping(), origin, Pose::at([-1.0, 0.0, 0.0]));
        stage.update(Some(Pose::IDENTITY), &space);
        let played = player.played()[0];
        assert_eq!(head_relative(&player.scene(), &played)[0], -1.0);
        space.place_point_source(id, looping(), origin, Pose::at([1.0, 0.0, 0.0]));
        stage.update(Some(Pose::IDENTITY), &space);
        assert_eq!(player.played().len(), 1);
        assert_eq!(head_relative(&player.scene(), &played)[0], 1.0);
    });
}

#[test]
fn a_source_that_leaves_ends_its_emitter_and_one_that_changes_its_sound_starts_again() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        let (kept, leaving) = (space.new_sound_id(), space.new_sound_id());
        let origin = Origin::from(Pose::IDENTITY);
        space.place_point_source(kept, looping(), origin, Pose::at([0.0, 0.0, -1.0]));
        space.place_point_source(leaving, looping(), origin, Pose::at([1.0, 0.0, -1.0]));
        stage.update(Some(Pose::IDENTITY), &space);
        let [first, second] = [0, 1].map(|index| emitter_of(&player.played()[index]));
        space.remove_point_source(leaving);
        stage.update(Some(Pose::IDENTITY), &space);
        assert_eq!(player.scene().read_emitter(second), EmitterReading::Gone);
        assert!(matches!(
            player.scene().read_emitter(first),
            EmitterReading::Placed { .. }
        ));
        // The same source with another sound: the old voice ends and a new one starts
        let other = PointSource::new(SoundId::from_raw(2)).looping();
        space.place_point_source(kept, other, origin, Pose::at([0.0, 0.0, -1.0]));
        stage.update(Some(Pose::IDENTITY), &space);
        assert_eq!(player.scene().read_emitter(first), EmitterReading::Gone);
        let played = player.played();
        assert_eq!(played.len(), 3);
        assert_eq!(played[2].sound, SoundId::from_raw(2));
    });
}

#[test]
fn a_distance_curve_reaches_the_emitter() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        let curve = DistanceAttenuation::Linear {
            min_distance: 1.0,
            max_distance: 4.0,
            gain_at_max_distance: 0.0,
        };
        let source = looping().params(PointSourceParams::new().distance_attenuation(curve));
        space.place_point_source(
            space.new_sound_id(),
            source,
            Origin::from(Pose::IDENTITY),
            Pose::at([0.0, 0.0, -2.0]),
        );
        stage.update(Some(Pose::IDENTITY), &space);
        let id = emitter_of(&player.played()[0]);
        assert!(matches!(
            player.scene().read_emitter(id),
            EmitterReading::Placed { attenuation, .. } if attenuation == curve
        ));
    });
}

#[test]
fn a_head_that_is_not_tracked_leaves_the_listener_where_it_was() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        stage.update(Some(Pose::at([1.0, 1.6, 2.0])), &space);
        stage.update(None, &space);
        let listener = player.scene().listener().expect("a listener");
        assert_eq!(listener.position, [1.0, 1.6, 2.0]);
    });
}

#[test]
fn a_following_subspace_carries_its_sound_with_the_head() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        let follow = space.new_follow_origin(FollowTarget::View(FollowMode::Tight));
        // A subspace of this kind stands a meter ahead of the head, so 1 m toward the viewer and
        // 0.8 m to the left puts the source level with the head, at its left
        space.place_point_source(
            space.new_sound_id(),
            looping(),
            Origin::following(follow),
            Pose::at([-0.8, 0.0, 1.0]),
        );
        let walk = [
            Pose::at([0.0; 3]),
            Pose::at([3.0, 0.0, 0.0]),
            turned_left(90.0),
        ];
        for head in walk {
            space.track(Some(head), [None, None], Duration::ZERO, None);
            stage.update(Some(head), &space);
            let played = player.played()[0];
            assert_eq!(
                head_relative(&player.scene(), &played),
                [-0.8, 0.0, 0.0],
                "the source stays at the head's left however the head moves"
            );
        }
        assert_eq!(player.played().len(), 1);
    });
}

#[test]
fn the_stage_is_silent_unless_the_app_has_input_focus() {
    let player = RecordingPlayer::new();
    let stage = stage(&player);
    let scene = player.scene();
    assert!(scene.is_paused(), "a new stage waits for the app's focus");
    let states = [
        (LifecycleState::Created, true),
        (LifecycleState::Started, true),
        (LifecycleState::Resumed, false),
        (LifecycleState::Paused, true),
        (LifecycleState::Resumed, false),
        (LifecycleState::Stopped, true),
        (LifecycleState::Resumed, false),
        (LifecycleState::Destroyed, true),
    ];
    for (state, silent) in states {
        stage.lifecycle(state);
        assert_eq!(scene.is_paused(), silent, "{state:?}");
    }
}

#[test]
fn a_player_with_no_spatial_scene_has_no_stage() {
    let player = Arc::new(RecordingPlayer {
        scene: None,
        played: Mutex::new(Vec::new()),
    });
    assert!(SoundStage::new(player).is_none());
    // An app that installed no audio engine has the no-op player behind the default one
    assert!(SoundStage::new(default_audio()).is_none());
}

#[test]
fn more_sources_than_emitters_play_as_many_as_there_are_emitters() {
    with_space(|space| {
        let player = RecordingPlayer::new();
        let mut stage = stage(&player);
        for index in 0..MAX_EMITTERS + 4 {
            space.place_point_source(
                space.new_sound_id(),
                looping(),
                Origin::from(Pose::IDENTITY),
                Pose::at([index as f32 * 0.1, 0.0, -1.0]),
            );
        }
        stage.update(Some(Pose::IDENTITY), &space);
        stage.update(Some(Pose::IDENTITY), &space);
        assert_eq!(player.played().len(), MAX_EMITTERS);
    });
}

// A headset whose content a test composes. It holds the space and a state that the content reads.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    shown: MutableState<bool>,
}

fn headset(content: impl Fn() + 'static) -> Headset {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let content = Rc::new(content);
    let mut robot = create_headless_robot_test(200, 200, move || {
        let space = remember(Space::new).with(Clone::clone);
        let shown = rememberMutableStateOf(|| true);
        keep.replace(Some((space.clone(), shown)));
        let content = Rc::clone(&content);
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            if shown.get() {
                content();
            }
        });
    });
    robot.wait_for_idle();
    let (space, shown) = held.take().expect("the content composed");
    Headset {
        robot,
        space,
        shown,
    }
}

fn places(space: &Space) -> Vec<SoundPlace> {
    let mut places = Vec::new();
    space.sounds_into(&mut places);
    places
}

#[test]
fn a_composable_source_stands_at_its_modifier_s_pose_while_it_is_in_the_composition() {
    let mut headset = headset(|| {
        SpatialPointSource(SubspaceModifier::new().offset(-800.0, 0.0, 0.0), looping());
    });
    let [place] = places(&headset.space)[..] else {
        panic!("one source");
    };
    // A subspace starts a meter ahead of the head's start. The offset is 800 dp, which are
    // millimeters, to the left.
    assert_eq!(place.pose.expect("a pose").position, [-0.8, 0.0, -1.0]);
    headset.shown.set(false);
    headset.robot.wait_for_idle();
    assert!(
        places(&headset.space).is_empty(),
        "the source left with its composable"
    );
}

#[test]
fn a_volume_that_carries_a_sound_takes_it_along_when_the_user_carries_the_volume() {
    let headset = headset(|| {
        SpatialVolume(
            SubspaceModifier::new()
                .size(300.0, 300.0)
                .depth(300.0)
                .movable()
                .point_source(looping()),
            |_| {},
        );
    });
    let panel = headset.space.panels()[0].id;
    assert_eq!(
        places(&headset.space)[0].pose.expect("a pose").position,
        [0.0, 0.0, -1.0]
    );
    let to = Pose::at([0.5, 0.2, -1.5]);
    headset.space.carry(&PanelMove {
        panel,
        event_type: SpatialMoveEventType::Moving,
        pose: to,
    });
    assert_eq!(places(&headset.space)[0].pose, Some(to));
}

#[test]
fn a_panel_with_a_sound_and_a_panel_without_one_differ_only_in_the_sound() {
    let headset = headset(|| {
        SpatialPanel(SubspaceModifier::new().point_source(looping()), || {});
        SpatialPanel(SubspaceModifier::new().offset(500.0, 0.0, 0.0), || {});
    });
    assert_eq!(headset.space.panels().len(), 2);
    assert_eq!(places(&headset.space).len(), 1);
}

#[test]
fn a_move_policy_that_leaves_the_pose_to_the_app_leaves_the_sound_with_it() {
    let headset = headset(|| {
        SpatialVolume(
            SubspaceModifier::new()
                .size(300.0, 300.0)
                .depth(300.0)
                .movable_with(MovePolicy::custom(|_| {}))
                .point_source(looping()),
            |_| {},
        );
    });
    let panel = headset.space.panels()[0].id;
    headset.space.carry(&PanelMove {
        panel,
        event_type: SpatialMoveEventType::Moving,
        pose: Pose::at([0.5, 0.2, -1.5]),
    });
    assert_eq!(
        places(&headset.space)[0].pose.expect("a pose").position,
        [0.0, 0.0, -1.0]
    );
}
