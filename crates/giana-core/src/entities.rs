use crate::collision::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyType {
    Monster,    // Authentic Stage 1 horned owl / monster ($92 / $93)
    RollingEye, // Rolling spiked eye ($C0)
    Crab,       // Ground crawler ($85..$88)
    Piranha,    // Pipe plant ($96)
    Dragon,     // Stage 33 boss ($B5)
}

#[derive(Debug, Clone)]
pub struct Enemy {
    pub enemy_type: EnemyType,
    pub x: f32,
    pub y: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    pub active: bool,
    pub is_dying: bool,
    pub death_timer: usize,
    pub anim_tick: usize,
    pub sprite_pointer: usize,
    pub hp: i32,
}

impl Enemy {
    pub fn new(enemy_type: EnemyType, x: f32, y: f32) -> Self {
        let (vel_x, ptr, hp) = match enemy_type {
            EnemyType::Monster => (-1.0, 0x92, 1),
            EnemyType::RollingEye => (-1.5, 0xB9, 1),
            EnemyType::Crab => (-1.0, 0x85, 1),
            EnemyType::Piranha => (0.0, 0x96, 1),
            EnemyType::Dragon => (-0.5, 0x60, 10),
        };

        Self {
            enemy_type,
            x,
            y,
            vel_x,
            vel_y: 0.0,
            active: true,
            is_dying: false,
            death_timer: 0,
            anim_tick: 0,
            sprite_pointer: ptr,
            hp,
        }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(self.x as i32, self.y as i32, 16, 16)
    }

    pub fn stomp(&mut self) {
        self.is_dying = true;
        self.death_timer = 15;
        self.vel_x = 0.0;
        self.vel_y = 0.0;
        self.sprite_pointer = 0x94; // Squashed flat frame
    }

    pub fn update(&mut self) {
        if !self.active {
            return;
        }

        if self.is_dying {
            if self.death_timer > 0 {
                self.death_timer -= 1;
            } else {
                self.active = false;
            }
            return;
        }

        self.anim_tick += 1;
        self.x += self.vel_x;
        self.y += self.vel_y;

        match self.enemy_type {
            EnemyType::Monster => {
                let frame_idx = (self.anim_tick / 8) % 2;
                self.sprite_pointer = 0x92 + frame_idx;
            }
            EnemyType::RollingEye => {
                let frame_idx = (self.anim_tick / 4) % 7;
                self.sprite_pointer = 0xB9 + frame_idx;
            }
            EnemyType::Crab => {
                let frame_idx = (self.anim_tick / 8) % 4;
                self.sprite_pointer = 0x85 + frame_idx;
            }
            EnemyType::Piranha => {
                self.sprite_pointer = 0x96;
                if self.anim_tick % 120 < 60 {
                    self.y -= 0.5;
                } else {
                    self.y += 0.5;
                }
            }
            EnemyType::Dragon => {
                let frame_idx = (self.anim_tick / 8) % 4;
                self.sprite_pointer = 0xB5 + frame_idx;
            }
        }
    }
}

/// Brick particle debris flying out when a block is shattered
#[derive(Debug, Clone)]
pub struct DebrisParticle {
    pub x: f32,
    pub y: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    pub active: bool,
}

impl DebrisParticle {
    pub fn new(x: f32, y: f32, vel_x: f32, vel_y: f32) -> Self {
        Self {
            x,
            y,
            vel_x,
            vel_y,
            active: true,
        }
    }

    pub fn update(&mut self) {
        if !self.active {
            return;
        }
        self.x += self.vel_x;
        self.y += self.vel_y;
        self.vel_y += 0.3; // Gravity
        if self.y > 260.0 {
            self.active = false;
        }
    }
}

/// Floating diamond or powerup item popping out of a hit block
#[derive(Debug, Clone)]
pub struct PowerupItem {
    pub x: f32,
    pub y: f32,
    pub vel_y: f32,
    pub is_diamond: bool,
    pub active: bool,
}

impl PowerupItem {
    pub fn new(x: f32, y: f32, is_diamond: bool) -> Self {
        Self {
            x,
            y,
            vel_y: -3.0, // Pops upward
            is_diamond,
            active: true,
        }
    }

    pub fn update(&mut self) {
        if !self.active {
            return;
        }
        self.y += self.vel_y;
        self.vel_y += 0.2; // Gravity
        if self.vel_y > 3.0 {
            self.active = false; // Collected / vanished
        }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(self.x as i32, self.y as i32, 16, 16)
    }
}

/// Projectile shot by Giana (Dream Bubble / Fireball)
#[derive(Debug, Clone)]
pub struct Projectile {
    pub x: f32,
    pub y: f32,
    pub vel_x: f32,
    pub active: bool,
    pub sprite_pointer: usize,
    pub popping: bool,
    pub pop_timer: usize,
}

impl Projectile {
    pub fn new(x: f32, y: f32, facing_right: bool) -> Self {
        Self {
            x,
            y,
            vel_x: if facing_right { 6.0 } else { -6.0 },
            active: true,
            sprite_pointer: 0xF7, // Authentic C64 dream bubble frame
            popping: false,
            pop_timer: 0,
        }
    }

    pub fn pop(&mut self) {
        self.popping = true;
        self.pop_timer = 6;
        self.sprite_pointer = 0xF8;
        self.vel_x = 0.0;
    }

    pub fn update(&mut self) {
        if !self.active {
            return;
        }
        if self.popping {
            if self.pop_timer > 0 {
                self.pop_timer -= 1;
            } else {
                self.active = false;
            }
            return;
        }
        self.x += self.vel_x;
        if self.x < 0.0 || self.x > 2100.0 {
            self.active = false;
        }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(self.x as i32, self.y as i32, 8, 8)
    }
}
