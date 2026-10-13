use smithay::backend::input::{
    AbsolutePositionEvent, Device, DeviceCapability, Event, InputBackend, InputTime, KeyState,
    KeyboardKeyEvent, Keycode, ProximityState, TabletToolAxisEvent, TabletToolCapabilities,
    TabletToolDescriptor, TabletToolEvent, TabletToolProximityEvent, TabletToolTipEvent,
    TabletToolTipState, TabletToolType, TouchCancelEvent, TouchDownEvent, TouchEvent,
    TouchFrameEvent, TouchMotionEvent, TouchSlot, TouchUpEvent, UnusedEvent,
};
use smithay::output::Output;

use crate::input::backend_ext::NiriInputDevice;

pub struct TestInputBackend;

#[derive(PartialEq, Eq, Hash)]
pub struct TestInputDevice;

pub struct TestKeyboardKeyEvent {
    pub time: InputTime,
    pub code: Keycode,
    pub state: KeyState,
    pub count: u32,
}

impl InputBackend for TestInputBackend {
    type Device = TestInputDevice;

    type KeyboardKeyEvent = TestKeyboardKeyEvent;
    type PointerAxisEvent = UnusedEvent;
    type PointerButtonEvent = UnusedEvent;
    type PointerMotionEvent = UnusedEvent;
    type PointerMotionAbsoluteEvent = UnusedEvent;

    type GestureSwipeBeginEvent = UnusedEvent;
    type GestureSwipeUpdateEvent = UnusedEvent;
    type GestureSwipeEndEvent = UnusedEvent;
    type GesturePinchBeginEvent = UnusedEvent;
    type GesturePinchUpdateEvent = UnusedEvent;
    type GesturePinchEndEvent = UnusedEvent;
    type GestureHoldBeginEvent = UnusedEvent;
    type GestureHoldEndEvent = UnusedEvent;

    type TouchDownEvent = TestTouchEvent;
    type TouchUpEvent = TestTouchEvent;
    type TouchMotionEvent = TestTouchEvent;
    type TouchCancelEvent = TestTouchEvent;
    type TouchFrameEvent = TestTouchEvent;
    type TabletToolAxisEvent = TestTabletEvent;
    type TabletToolProximityEvent = TestTabletEvent;
    type TabletToolTipEvent = TestTabletEvent;
    type TabletToolButtonEvent = UnusedEvent;

    type SwitchToggleEvent = UnusedEvent;

    type SpecialEvent = UnusedEvent;
}

impl Device for TestInputDevice {
    fn id(&self) -> String {
        String::from("test")
    }

    fn name(&self) -> String {
        String::from("test input device")
    }

    fn has_capability(&self, capability: DeviceCapability) -> bool {
        matches!(capability, DeviceCapability::Keyboard)
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

impl NiriInputDevice for TestInputDevice {
    fn output(&self, _state: &crate::niri::State) -> Option<Output> {
        None
    }
}

impl Event<TestInputBackend> for TestKeyboardKeyEvent {
    fn time(&self) -> InputTime {
        self.time
    }

    fn device(&self) -> <TestInputBackend as InputBackend>::Device {
        TestInputDevice
    }
}

impl KeyboardKeyEvent<TestInputBackend> for TestKeyboardKeyEvent {
    fn key_code(&self) -> Keycode {
        self.code
    }

    fn state(&self) -> KeyState {
        self.state
    }

    fn count(&self) -> u32 {
        self.count
    }
}

// Normalized positions are transformed by the production device input dispatch.
#[derive(Clone)]
pub struct TestTouchEvent {
    pub time: InputTime,
    pub position: (f64, f64),
    pub slot: TouchSlot,
}
impl TestTouchEvent {
    pub fn new(position: (f64, f64)) -> Self {
        Self {
            time: InputTime::now(),
            position,
            slot: Some(0).into(),
        }
    }
}
impl Event<TestInputBackend> for TestTouchEvent {
    fn time(&self) -> InputTime {
        self.time
    }
    fn device(&self) -> TestInputDevice {
        TestInputDevice
    }
}
impl TouchEvent<TestInputBackend> for TestTouchEvent {
    fn slot(&self) -> TouchSlot {
        self.slot
    }
}
impl TouchDownEvent<TestInputBackend> for TestTouchEvent {}
impl TouchMotionEvent<TestInputBackend> for TestTouchEvent {}
impl TouchUpEvent<TestInputBackend> for TestTouchEvent {}
impl TouchCancelEvent<TestInputBackend> for TestTouchEvent {}
impl TouchFrameEvent<TestInputBackend> for TestTouchEvent {}

#[derive(Clone)]
pub struct TestTabletEvent {
    pub time: InputTime,
    pub position: (f64, f64),
    pub tool: TabletToolDescriptor,
    pub proximity: ProximityState,
    pub tip: TabletToolTipState,
}
impl TestTabletEvent {
    pub fn new(position: (f64, f64)) -> Self {
        Self {
            time: InputTime::now(),
            position,
            tool: TabletToolDescriptor {
                tool_type: TabletToolType::Pen,
                hardware_serial: 72,
                hardware_id_wacom: 72,
                capabilities: TabletToolCapabilities::empty(),
            },
            proximity: ProximityState::In,
            tip: TabletToolTipState::Down,
        }
    }
}
impl Event<TestInputBackend> for TestTabletEvent {
    fn time(&self) -> InputTime {
        self.time
    }
    fn device(&self) -> TestInputDevice {
        TestInputDevice
    }
}

impl AbsolutePositionEvent<TestInputBackend> for TestTouchEvent {
    fn x(&self) -> f64 {
        self.position.0
    }
    fn y(&self) -> f64 {
        self.position.1
    }
    fn x_transformed(&self, width: i32) -> f64 {
        self.position.0 * f64::from(width)
    }
    fn y_transformed(&self, height: i32) -> f64 {
        self.position.1 * f64::from(height)
    }
}

impl AbsolutePositionEvent<TestInputBackend> for TestTabletEvent {
    fn x(&self) -> f64 {
        self.position.0
    }
    fn y(&self) -> f64 {
        self.position.1
    }
    fn x_transformed(&self, width: i32) -> f64 {
        self.position.0 * f64::from(width)
    }
    fn y_transformed(&self, height: i32) -> f64 {
        self.position.1 * f64::from(height)
    }
}

impl TabletToolEvent<TestInputBackend> for TestTabletEvent {
    fn tool(&self) -> TabletToolDescriptor {
        self.tool.clone()
    }
    fn delta_x(&self) -> f64 {
        0.
    }
    fn delta_y(&self) -> f64 {
        0.
    }
    fn distance(&self) -> f64 {
        0.
    }
    fn distance_has_changed(&self) -> bool {
        false
    }
    fn pressure(&self) -> f64 {
        0.
    }
    fn pressure_has_changed(&self) -> bool {
        false
    }
    fn slider_position(&self) -> f64 {
        0.
    }
    fn slider_has_changed(&self) -> bool {
        false
    }
    fn tilt_x(&self) -> f64 {
        0.
    }
    fn tilt_x_has_changed(&self) -> bool {
        false
    }
    fn tilt_y(&self) -> f64 {
        0.
    }
    fn tilt_y_has_changed(&self) -> bool {
        false
    }
    fn rotation(&self) -> f64 {
        0.
    }
    fn rotation_has_changed(&self) -> bool {
        false
    }
    fn wheel_delta(&self) -> f64 {
        0.
    }
    fn wheel_delta_discrete(&self) -> i32 {
        0
    }
    fn wheel_has_changed(&self) -> bool {
        false
    }
}
impl TabletToolAxisEvent<TestInputBackend> for TestTabletEvent {}
impl TabletToolProximityEvent<TestInputBackend> for TestTabletEvent {
    fn state(&self) -> ProximityState {
        self.proximity
    }
}
impl TabletToolTipEvent<TestInputBackend> for TestTabletEvent {
    fn tip_state(&self) -> TabletToolTipState {
        self.tip
    }
}
