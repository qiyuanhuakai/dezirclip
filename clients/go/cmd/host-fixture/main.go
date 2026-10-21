// host-fixture exercises the same client against a real Core-backed Host. It
// reads synthetic metadata only and never invokes native clipboard operations.
package main
import (
    "context"
    "encoding/json"
    "fmt"
    "os"
    "time"
    "github.com/qiyuanhuakai/dezirclip/clients/go/client"
    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)
func run()error {
    if len(os.Args)!=3 {return fmt.Errorf("fixture endpoint and expected mode required")}
    ctx,cancel:=context.WithTimeout(context.Background(),10*time.Second);defer cancel()
    hello:=protocol.HelloParams{ProtocolVersion:protocol.Version,ClientVersion:"actual-host-fixture",ClientRole:protocol.ClientRoleSurface,Capabilities:[]string{},RequiredCapabilities:[]string{}}
    connection,err:=client.Dial(ctx,os.Args[1],hello);if err!=nil{return err};defer connection.Close()
    var status protocol.ServiceStatus
    if err=connection.Call(ctx,protocol.MethodServiceStatus,protocol.EmptyParams{},"ServiceStatus",&status);err!=nil{return err}
    if string(status.Mode)!=os.Args[2] {return client.ErrProtocol}
    var listed protocol.HistoryResult
    if err=connection.Call(ctx,protocol.MethodSurfaceHistoryList,protocol.HistoryPageParams{Limit:100},"HistoryResult",&listed);err!=nil{return err}
    if len(listed.Entries)==0 {return client.ErrProtocol}
    var events protocol.EventsResult
    wait:=uint16(0)
    if err=connection.Call(ctx,protocol.MethodEventsSubscribe,protocol.SubscribeParams{AfterSequence:0,WaitMs:&wait},"EventsResult",&events);err!=nil{return err}
    cursor:=client.Cursor{}
    if !cursor.Observe(events) {return client.ErrProtocol}
    second,err:=client.Dial(ctx,os.Args[1],hello);if err!=nil{return err};defer second.Close()
    if second.Hello().State.ServiceInstanceId!=connection.Hello().State.ServiceInstanceId {return client.ErrProtocol}
    report:=map[string]any{"mode":status.Mode,"state":status.State,"entries":len(listed.Entries),"resync_required":events.ResyncRequired,"same_client_reconnect":true}
    return json.NewEncoder(os.Stdout).Encode(report)
}
func main(){if err:=run();err!=nil{fmt.Fprintln(os.Stderr,err);os.Exit(1)}}
