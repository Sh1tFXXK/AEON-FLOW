use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, canvas::{Canvas, Points}},
    style::{Color, Style},
    text::Span,
    Terminal,
};
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures_util::StreamExt;
use std::io::stdout;
use std::time::{Duration, Instant};
use tokio_tungstenite::connect_async;
use rand::Rng;

struct Particle {
    id: String,
    event_type: String,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    color: Color,
}

impl Particle {
    fn update(&mut self) {
        self.x += self.vx;
        self.y += self.vy;

        // Bounce off bounds [0, 100]
        if self.x < 0.0 || self.x > 100.0 {
            self.vx = -self.vx;
            self.x = self.x.clamp(0.0, 100.0);
        }
        if self.y < 0.0 || self.y > 100.0 {
            self.vy = -self.vy;
            self.y = self.y.clamp(0.0, 100.0);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let url = "ws://127.0.0.1:8080";
    let (ws_stream, _) = match connect_async(url).await {
        Ok(v) => v,
        Err(_) => {
            disable_raw_mode()?;
            std::io::stdout().execute(LeaveAlternateScreen)?;
            eprintln!("Failed to connect to nebula daemon at {}. Is it running?", url);
            return Ok(());
        }
    };
    let (_, mut read) = ws_stream.split();

    let mut particles: Vec<Particle> = Vec::new();
    let mut rng = rand::thread_rng();
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(33); // ~30 FPS

    loop {
        terminal.draw(|f| {
            let area = f.size();
            
            let canvas = Canvas::default()
                .block(Block::default().title(" Nebula Data Universe ").borders(Borders::ALL))
                .x_bounds([0.0, 100.0])
                .y_bounds([0.0, 100.0])
                .paint(|ctx| {
                    for p in &particles {
                        ctx.draw(&Points {
                            coords: &[(p.x, p.y)],
                            color: p.color,
                        });
                        ctx.print(p.x, p.y + 2.0, Span::styled(
                            format!("{} ({})", p.id, p.event_type),
                            Style::default().fg(p.color)
                        ));
                    }
                });

            f.render_widget(canvas, area);
        })?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            for p in &mut particles {
                p.update();
            }
            last_tick = Instant::now();
        }

        // Try to read one message per loop iteration if available
        if let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(1), read.next()).await {
            if let Ok(text) = msg.to_text() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                    let id = v["id"].as_str().unwrap_or("unknown").to_string();
                    let etype = v["type"].as_str().unwrap_or("UNKNOWN").to_string();
                    
                    let color = match etype.as_str() {
                        "VM_CREATED" => Color::Cyan,
                        "VM_MIGRATED" => Color::Blue,
                        "VM_TERMINATED" => Color::Red,
                        _ => Color::White,
                    };

                    particles.push(Particle {
                        id,
                        event_type: etype,
                        x: rng.gen_range(10.0..90.0),
                        y: rng.gen_range(10.0..90.0),
                        vx: rng.gen_range(-1.5..1.5),
                        vy: rng.gen_range(-1.5..1.5),
                        color,
                    });
                    if particles.len() > 50 {
                        particles.remove(0);
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    std::io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
