#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QCloseEvent>
#include <QComboBox>
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFrame>
#include <QGuiApplication>
#include <QGraphicsOpacityEffect>
#include <QHBoxLayout>
#include <QIcon>
#include <QImage>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QPainter>
#include <QPainterPath>
#include <QProcess>
#include <QPushButton>
#include <QRegularExpression>
#include <QScreen>
#include <QSettings>
#include <QSlider>
#include <QStandardPaths>
#include <QSystemTrayIcon>
#include <QTextStream>
#include <QTimer>
#include <QVBoxLayout>
#include <QVariantAnimation>
#include <QWidget>
#include <QWindow>
#include <cmath>

class MonitorArt final : public QWidget {
public:
    explicit MonitorArt(QWidget *p=nullptr):QWidget(p){setMinimumHeight(250);setSizePolicy(QSizePolicy::Expanding,QSizePolicy::Expanding);setAttribute(Qt::WA_OpaquePaintEvent);timer.setInterval(70);connect(&timer,&QTimer::timeout,this,[this]{phase+=0.018;update();});}
    void animate(bool on){if(on)timer.start();else timer.stop();}
    void setPacket(const QByteArray &packet){
        constexpr int imageBytes=320*180*3, packetBytes=imageBytes+65*3;
        if(packet.size()!=packetBytes)return;
        image=QImage(reinterpret_cast<const uchar*>(packet.constData()),320,180,320*3,QImage::Format_RGB888).copy();hasFrame=true;
        for(int i=0;i<65;++i)leds[i]=QColor((uchar)packet[imageBytes+i*3],(uchar)packet[imageBytes+i*3+1],(uchar)packet[imageBytes+i*3+2]);
        update();
    }
protected:
    void paintEvent(QPaintEvent*) override {
        QPainter p(this);p.setRenderHint(QPainter::Antialiasing);p.fillRect(rect(),QColor("#11161b"));
        const qreal screenH=std::min(height()*0.90,width()*0.88*9.0/16.0);
        const qreal screenW=screenH*16.0/9.0;
        QRectF screen=QRectF((width()-screenW)/2,(height()-screenH)/2-4,screenW,screenH);
        QRectF frame=screen.adjusted(-8,-8,8,8);
        QLinearGradient halo(frame.topLeft(),frame.topRight());halo.setColorAt(0,hasFrame?QColor(32,224,238):QColor("#56606a"));halo.setColorAt(.5,hasFrame?QColor(168,88,255):QColor("#56606a"));halo.setColorAt(1,hasFrame?QColor(255,174,68):QColor("#56606a"));
        QColor glow=halo.stops().at(0).second;glow.setAlpha(65+int(18*std::sin(phase)));
        if(hasFrame){p.setPen(QPen(glow,15));p.drawRoundedRect(frame,10,10);}
        p.setPen(QPen(halo,3));p.drawRoundedRect(frame,10,10);
        p.setPen(Qt::NoPen);if(!image.isNull())p.drawImage(screen,image);else{p.setBrush(QColor("#171c22"));p.drawRoundedRect(screen,4,4);p.setPen(QColor("#89919c"));p.drawText(screen,Qt::AlignCenter,"Aguardando captura real");p.setPen(Qt::NoPen);}
        p.setPen(Qt::NoPen);
        for(int i=0;i<31;i++){QRectF seg(screen.left()+i*screen.width()/31.0,screen.top()-8,screen.width()/31.0-2,5);p.setBrush(leds[i+17]);p.drawRoundedRect(seg,2,2);}
        for(int i=0;i<17;i++){QRectF l(screen.left()-8,screen.top()+i*screen.height()/17.0,5,screen.height()/17.0-2);p.setBrush(leds[i]);p.drawRoundedRect(l,2,2);QRectF r(screen.right()+3,screen.top()+i*screen.height()/17.0,5,screen.height()/17.0-2);p.setBrush(leds[i+48]);p.drawRoundedRect(r,2,2);}
        p.setBrush(QColor("#0c1117"));p.drawRoundedRect(QRectF(width()*.47,height()*.85,width()*.06,height()*.065),2,2);p.drawRoundedRect(QRectF(width()*.37,height()*.91,width()*.26,height()*.025),3,3);
    }
private: QTimer timer;double phase=0;QImage image;QColor leds[65]{};bool hasFrame=false;
};

class Panel final : public QWidget {
    Q_OBJECT
public:
    Panel(){
        const auto previousAlgorithm=QSettings().value("algorithm").toString();if(previousAlgorithm.isEmpty()||previousAlgorithm=="nearest"||previousAlgorithm=="box"){QSettings().setValue("algorithm","grid");QSettings().setValue("velocity",100);}
        setWindowTitle("SyncLights");setMinimumSize(950,650);resize(QSettings().value("windowSize",QSize(1080,720)).toSize());
        setStyleSheet(R"(
          QWidget#root{background:#111419;color:#e9e5d8;font-family:'Noto Sans','Inter';}
          QLabel{background:transparent;color:#e9e5d8;} QLabel.muted{color:#89919c;font-size:11px;letter-spacing:1px;}
          QLabel.big{font-size:28px;font-weight:700;} QLabel.metric{font-size:34px;font-weight:700;color:#c5f05a;}
          QComboBox{background:#191e24;color:#e9e5d8;border:1px solid #3b434d;border-radius:6px;padding:10px 12px;min-height:22px;}
          QComboBox QAbstractItemView{background:#191e24;color:#e9e5d8;selection-background-color:#343d2a;selection-color:#c5f05a;}
          QSlider::groove:horizontal{height:5px;background:#454d57;border-radius:2px;} QSlider::sub-page:horizontal{background:#c5f05a;border-radius:2px;} QSlider::handle:horizontal{background:#c5f05a;width:17px;height:17px;margin:-6px 0;border-radius:8px;}
          QCheckBox{spacing:9px;color:#e9e5d8;} QCheckBox::indicator{width:18px;height:18px;border:1px solid #7b8490;border-radius:4px;background:#191e24;} QCheckBox::indicator:checked{background:#c5f05a;border-color:#c5f05a;}
          QPushButton{color:#e9e5d8;background:#191e24;border:1px solid #414953;border-radius:8px;padding:12px 14px;font-weight:600;} QPushButton:hover{border-color:#c5f05a;background:#222820;} QPushButton:checked{border:2px solid #c5f05a;background:#283021;color:#c5f05a;}
          QPushButton#primary{background:#c5f05a;color:#171b12;border:0;font-size:15px;font-weight:700;padding:15px 24px;} QPushButton#primary:hover{background:#d2fa6c;}
          QFrame#divider{background:#343b44;max-width:1px;} QFrame#card{background:#191e24;border:1px solid #343b44;border-radius:10px;}
        )");
        auto *root=new QHBoxLayout(this);root->setContentsMargins(0,0,0,0);root->setSpacing(0);
        auto *side=new QWidget;side->setFixedWidth(154);side->setStyleSheet("background:#0d1116;border-right:1px solid #303740;");auto *sl=new QVBoxLayout(side);sl->setContentsMargins(13,18,13,18);sl->setSpacing(13);
        auto *brand=new QLabel;brand->setPixmap(QIcon(SYNCLIGHTS_ICON).pixmap(32,32));brand->setFixedHeight(42);sl->addWidget(brand);
        auto *word=new QLabel("SYNC\nLIGHTS");word->setStyleSheet("font-size:19px;font-weight:800;letter-spacing:2px;color:#c5f05a;");sl->addWidget(word);
        auto *nav=new QPushButton("▣   Tela principal");nav->setCheckable(true);nav->setChecked(true);nav->setStyleSheet("text-align:left;padding:13px 9px;");sl->addWidget(nav);
        auto *settingsNav=new QPushButton("☷   Ajustes");settingsNav->setStyleSheet("text-align:left;padding:13px 9px;");sl->addWidget(settingsNav);sl->addStretch();
        auto *version=new QLabel("MOTOR NATIVO\nRust · X11");version->setStyleSheet("color:#707985;font-size:10px;letter-spacing:1px;");sl->addWidget(version);root->addWidget(side);
        auto *main=new QWidget;auto *ml=new QVBoxLayout(main);ml->setContentsMargins(24,20,24,18);ml->setSpacing(12);
        auto *pageTitle=new QLabel("Tela principal");pageTitle->setObjectName("title");pageTitle->setMinimumHeight(46);pageTitle->setAlignment(Qt::AlignLeft|Qt::AlignVCenter);pageTitle->setStyleSheet("color:#e9e5d8;font-size:28px;font-weight:700;");ml->addWidget(pageTitle);
        preview=new MonitorArt;ml->addWidget(preview,1);
        auto *runRow=new QHBoxLayout;status=new QLabel("●  Pronta para sincronizar");status->setStyleSheet("border:1px solid #596344;border-radius:20px;padding:12px 17px;color:#c5f05a;font-weight:600;");runRow->addWidget(status);runRow->addStretch();toggle=new QPushButton("▶   Iniciar sincronização");toggle->setObjectName("primary");toggle->setMinimumWidth(240);runRow->addWidget(toggle);ml->addLayout(runRow);
        auto *modes=new QHBoxLayout;modes->setSpacing(11);game=modeCard("Jogo","grid",100,150,120);film=modeCard("Filme","gaussian",45,95,92);ambient=modeCard("Ambiente","luma",70,110,100);const auto savedMode=QSettings().value("algorithm","grid").toString();game->setChecked(savedMode=="grid");film->setChecked(savedMode=="gaussian");ambient->setChecked(savedMode=="luma");modes->addWidget(game);modes->addWidget(film);modes->addWidget(ambient);ml->addLayout(modes);
        auto *controls=new QHBoxLayout;controls->setSpacing(14);controls->addWidget(sliderGroup("Intensidade",strength,0,200,100));controls->addWidget(sliderGroup("Contraste",contrast,50,200,100));controls->addWidget(sliderGroup("Velocidade",fps,0,100,100));invert=new QCheckBox("Inverter fita");invert->setChecked(QSettings().value("invert",false).toBool());controls->addWidget(invert);ml->addLayout(controls);root->addWidget(main,1);
        auto *rail=new QWidget;rail->setFixedWidth(205);rail->setStyleSheet("background:#14191f;border-left:1px solid #303740;");auto *rl=new QVBoxLayout(rail);rl->setContentsMargins(15,25,15,20);rl->setSpacing(11);
        auto *monLabel=new QLabel("Monitor");monLabel->setStyleSheet("font-size:18px;font-weight:700;");rl->addWidget(monLabel);monitor=new QComboBox;populateMonitors();monitor->setCurrentIndex(monitor->findData(QSettings().value("monitor",0).toInt()));rl->addWidget(monitor);
        auto *algoLabel=new QLabel("MÉTODO DE AMOSTRAGEM");algoLabel->setStyleSheet("color:#89919c;font-size:10px;letter-spacing:1px;margin-top:17px;");rl->addWidget(algoLabel);
        estimator=new QComboBox;estimator->addItem("Média rápida 8×8 (64 amostras)","grid");estimator->addItem("Média integral da área","box");estimator->addItem("Gaussiano 3×3","gaussian");estimator->addItem("Peso por luminância","luma");estimator->addItem("Mediana 3×3 · ruído","median");estimator->addItem("Reduzir p/ 320×180","resize");estimator->addItem("Pixel único · diagnóstico","nearest");estimator->setCurrentIndex(std::max(0,estimator->findData(QSettings().value("algorithm","grid"))));rl->addWidget(estimator);
        auto *line=new QFrame;line->setObjectName("divider");line->setFrameShape(QFrame::VLine);rl->addWidget(line);
        auto *latLabel=new QLabel("MOTOR → USB · P95");latLabel->setProperty("class","muted");latLabel->setStyleSheet("color:#89919c;font-size:10px;letter-spacing:1px;");rl->addWidget(latLabel);latency=new QLabel("— ms");latency->setProperty("class","metric");latency->setStyleSheet("font-size:32px;font-weight:700;color:#c5f05a;");rl->addWidget(latency);
        auto *fpsLabel=new QLabel("CAPTURA REAL · META 120");fpsLabel->setStyleSheet("color:#89919c;font-size:10px;letter-spacing:1px;margin-top:20px;");rl->addWidget(fpsLabel);actualFps=new QLabel("— FPS");actualFps->setStyleSheet("font-size:28px;font-weight:700;color:#c5f05a;");rl->addWidget(actualFps);
        auto *hint=new QLabel("Captura sempre no máximo (até 120 FPS). Velocidade ajusta só a transição entre cenas. Jogo reage na hora; Filme suaviza sem reduzir a captura.");hint->setWordWrap(true);hint->setStyleSheet("color:#89919c;font-size:11px;margin-top:12px;");rl->addWidget(hint);rl->addStretch();autostart=new QCheckBox("Iniciar com o sistema");autostart->setChecked(QSettings().value("autostart",false).toBool());rl->addWidget(autostart);root->addWidget(rail);
        connect(toggle,&QPushButton::clicked,this,[this]{running?stop():start();});
        connect(settingsNav,&QPushButton::clicked,this,[this]{QMessageBox::information(this,"Ajustes do motor",QString("Captura contínua até 120 FPS no monitor %1.\n\nAmostragem: %2. Velocidade só controla a transição temporal, sem reduzir a captura.\n\nMétrica: captura→USB no host (P95); não inclui o processamento físico da fita.\n\nFita: protocolo HID ROBOBLOQ, interface 0. A regra udev permite acesso sem sudo.\n\nJogo prioriza resposta, contraste e intensidade; Filme suaviza as transições; Ambiente usa média ponderada por luminância.").arg(monitor->currentText(),estimator->currentText()));});
        latency->setToolTip("Tempo de software entre ler o frame e concluir a escrita HID. Não mede o tempo físico até os LEDs.");
        connect(game,&QPushButton::clicked,this,[this]{applyMode("grid",100,150,120,game);});connect(film,&QPushButton::clicked,this,[this]{applyMode("gaussian",45,95,92,film);});connect(ambient,&QPushButton::clicked,this,[this]{applyMode("luma",70,110,100,ambient);});
        restartTimer.setSingleShot(true);restartTimer.setInterval(180);connect(&restartTimer,&QTimer::timeout,this,[this]{if(running)start();});
        connect(estimator,qOverload<int>(&QComboBox::currentIndexChanged),this,[this](int i){if(i<0)return;const auto method=estimator->itemData(i).toString();QSettings().setValue("algorithm",method);game->setChecked(method=="grid");film->setChecked(method=="gaussian");ambient->setChecked(method=="luma");if(running&&!applyingMode)start();});
        connect(monitor,qOverload<int>(&QComboBox::currentIndexChanged),this,[this](int i){QSettings().setValue("monitor",monitor->itemData(i).toInt());if(running)start();});
        connect(strength,&QSlider::valueChanged,this,[this](int v){QSettings().setValue("strength",v);if(running&&!applyingMode)restartTimer.start();});connect(contrast,&QSlider::valueChanged,this,[this](int v){QSettings().setValue("contrast",v);if(running&&!applyingMode)restartTimer.start();});connect(fps,&QSlider::valueChanged,this,[this](int v){QSettings().setValue("velocity",v);if(running&&!applyingMode)restartTimer.start();});connect(invert,&QCheckBox::toggled,this,[this](bool v){QSettings().setValue("invert",v);if(running)start();});connect(autostart,&QCheckBox::toggled,this,[this](bool v){QSettings().setValue("autostart",v);setAutostart(v);});
        connect(&worker,&QProcess::readyReadStandardError,this,[this]{const auto log=QString::fromUtf8(worker.readAllStandardError());const QRegularExpression re("capture_fps=([0-9.]+).*frame_read_to_hid_write_p95=([0-9.]+)ms");auto m=re.match(log);if(m.hasMatch()){actualFps->setText(QString("%1 FPS").arg(m.captured(1)));latency->setText(QString("%1 ms").arg(QString::number(m.captured(2).toDouble(),'f',0)));}else if(log.contains("frame_read_to_hid_write_p95=n/a")){latency->setText("— ms");}if(log.contains("sync failed:")||log.contains("capture stopped:")){status->setText("Falha · confira captura/USB");}});
        connect(&worker,&QProcess::readyReadStandardOutput,this,[this]{constexpr int packet=320*180*3+65*3;previewBuffer.append(worker.readAllStandardOutput());while(previewBuffer.size()>=packet){preview->setPacket(previewBuffer.left(packet));previewBuffer.remove(0,packet);}});
        connect(&worker,&QProcess::errorOccurred,this,[this]{status->setText("Falha ao iniciar · confira FFmpeg e a fita");});connect(&worker,qOverload<int,QProcess::ExitStatus>(&QProcess::finished),this,[this]{running=false;setRunningUi(false);});
        setStyleSheet(styleSheet()+" QWidget#root{background:#111419;}");
    }
    QPushButton *modeCard(QString text,QString algo,int rate,int c,int s){auto*b=new QPushButton(text);b->setCheckable(true);b->setProperty("algorithm",algo);b->setProperty("rate",rate);b->setProperty("contrast",c);b->setProperty("strength",s);return b;}
    QWidget *sliderGroup(QString title,QSlider *&out,int lo,int hi,int def){auto*w=new QWidget;auto*l=new QVBoxLayout(w);l->setContentsMargins(0,0,0,0);auto*head=new QHBoxLayout;auto*label=new QLabel(title);label->setStyleSheet("font-size:12px;font-weight:600;");auto*value=new QLabel;value->setStyleSheet("font-size:10px;color:#89919c;");head->addWidget(label);head->addStretch();head->addWidget(value);l->addLayout(head);out=new QSlider(Qt::Horizontal);out->setRange(lo,hi);const QString key=title=="Intensidade"?"strength":title=="Contraste"?"contrast":"velocity";out->setValue(QSettings().value(key,def).toInt());auto updateValue=[value,title](int v){value->setText(title=="Velocidade"?(v==100?"MAX · IMEDIATA":QString("TRANSIÇÃO %1 ms").arg((100-v)*1.5,0,'f',0)):QString("%1%").arg(v));};updateValue(out->value());connect(out,&QSlider::valueChanged,w,updateValue);l->addWidget(out);return w;}
    void populateMonitors(){QProcess x;x.start("xrandr",{"--listmonitors"});x.waitForFinished(1000);QRegularExpression re("^\\s*(\\d+):\\s+\\+\\*?([\\w-]+)\\s+(\\d+)/\\d+x(\\d+)/\\d+");for(const auto&line:QString::fromUtf8(x.readAllStandardOutput()).split('\n')){auto m=re.match(line);if(m.hasMatch())monitor->addItem(QString("%1 · %2 × %3").arg(m.captured(2),m.captured(3),m.captured(4)),m.captured(1).toInt());}if(monitor->count()==0)monitor->addItem("Tela principal",0);}
    void applyMode(QString a,int velocity,int c,int s,QPushButton*b){applyingMode=true;for(auto*x:{game,film,ambient})x->setChecked(x==b);estimator->setCurrentIndex(estimator->findData(a));QSettings().setValue("algorithm",a);fps->setValue(velocity);contrast->setValue(c);strength->setValue(s);applyingMode=false;if(running)start();}
    bool moveAwayFromCapture(){const QString target=monitor->currentText().section(' ',0,0);QScreen*selected=nullptr;for(auto*s:QGuiApplication::screens())if(s->name()==target){selected=s;break;}if(!selected||!windowHandle()||windowHandle()->screen()!=selected)return false;for(auto*s:QGuiApplication::screens())if(s!=selected){move(s->availableGeometry().center()-rect().center());return true;}hide();return true;}
    void setRunningUi(bool on){running=on;preview->animate(on);toggle->setText(on?"■   Parar sincronização":"▶   Iniciar sincronização");status->setText(on?(windowMoved?"●  Sincronização ativa · painel na outra tela":"●  Sincronização ativa"):"●  Pronta para sincronizar");}
    void start(){restartTimer.stop();if(running){worker.kill();worker.waitForFinished(700);running=false;}windowMoved=moveAwayFromCapture();const QString exe=QDir(QCoreApplication::applicationDirPath()).filePath("synclights");worker.start(exe,{"sync","auto",monitor->currentData().toString(),"120",QSettings().value("algorithm","grid").toString(),invert->isChecked()?"invert":"normal",QString::number(contrast->value()/100.0,'f',2),QString::number(strength->value()/100.0,'f',2),"preview",QString::number((100-fps->value())*1.5,'f',1)});setRunningUi(true);}
    void stop(){worker.terminate();if(!worker.waitForFinished(500))worker.kill();setRunningUi(false);}
    void setAutostart(bool yes){const QString dir=QStandardPaths::writableLocation(QStandardPaths::ConfigLocation)+"/autostart";QDir().mkpath(dir);const QString path=dir+"/synclights.desktop";if(!yes){QFile::remove(path);return;}QFile f(path);if(f.open(QIODevice::WriteOnly|QIODevice::Text)){QTextStream o(&f);o<<"[Desktop Entry]\nType=Application\nName=SyncLights\nExec=\""<<QCoreApplication::applicationFilePath()<<"\" --start\nIcon=synclights\n";}}
    void closeEvent(QCloseEvent*e)override{QSettings().setValue("windowSize",size());hide();e->ignore();}
    MonitorArt*preview{};QComboBox*monitor{},*estimator{};QSlider*strength{},*contrast{},*fps{};QCheckBox*invert{},*autostart{};QPushButton*toggle{},*game{},*film{},*ambient{};QLabel*status{},*latency{},*actualFps{};QProcess worker;QTimer restartTimer;QByteArray previewBuffer;bool running=false,applyingMode=false,windowMoved=false;
};

int main(int argc,char**argv){QApplication app(argc,argv);QApplication::setApplicationName("SyncLights");QApplication::setOrganizationName("edersonff");app.setQuitOnLastWindowClosed(false);Panel panel;QSystemTrayIcon tray(QIcon(SYNCLIGHTS_ICON));tray.setToolTip("SyncLights · sincronização de tela");QMenu menu;auto*show=menu.addAction("Abrir SyncLights");auto*toggle=menu.addAction("Iniciar sincronização");menu.addSeparator();menu.addAction("Sair",&app,&QApplication::quit);QObject::connect(show,&QAction::triggered,&panel,&QWidget::showNormal);QObject::connect(toggle,&QAction::triggered,&panel,[&panel,toggle]{if(panel.running){panel.stop();toggle->setText("Iniciar sincronização");}else{panel.start();toggle->setText("Parar sincronização");}});QObject::connect(&tray,&QSystemTrayIcon::activated,&panel,[&panel](auto reason){if(reason==QSystemTrayIcon::Trigger)panel.showNormal();});QObject::connect(&app,&QApplication::aboutToQuit,&panel,[&panel]{if(panel.running)panel.stop();});QTimer trayState;trayState.setInterval(500);QObject::connect(&trayState,&QTimer::timeout,&app,[&panel,toggle]{toggle->setText(panel.running?"Parar sincronização":"Iniciar sincronização");});trayState.start();tray.setContextMenu(&menu);tray.show();const auto args=QCoreApplication::arguments();if(args.contains("--open"))panel.show();if(args.contains("--start"))QTimer::singleShot(800,&panel,[&panel]{panel.start();});return app.exec();}
#include "main.moc"
